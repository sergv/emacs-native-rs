// #[cfg(test)]
// mod tests {
//     #[test]
//     fn it_works() {
//         assert_eq!(2 + 2, 4);
//     }
// }

use std::iter::IntoIterator;
use std::mem::MaybeUninit;
use std::path::{PathBuf};
use std::result;
use std::sync::Barrier;
use std::sync::mpsc;

// use crossbeam_channel::bounded;
use crossbeam::queue::ArrayQueue;

use crossbeam;
use crossbeam::thread::ScopedJoinHandle;
use globset::{Glob, GlobSet, GlobBuilder, GlobSetBuilder};

use emacs::{defun, Env, Result, Value, Vector, FromLisp, IntoLisp};


emacs::use_symbols!(nil);

// Emacs won't load the module without this.
emacs::plugin_is_GPL_compatible!();

// Register the initialization hook that Emacs will call when it loads the module.
#[emacs::module(name = "emacs_native")]
fn init(env: &Env) -> Result<Value<'_>> {
    ().into_lisp(env)
}

fn mk_glob(pat: &str) -> result::Result<Glob, globset::Error> {
    let mut b = GlobBuilder::new(pat);
    b.case_insensitive(true);
    b.literal_separator(false);
    b.backslash_escape(false);
    b.build()
}


fn glob_should_test_against_abs(pat: &str) -> bool {
    pat.chars().any(std::path::is_separator)
    // std::path::Path::new(pat).is_absolute()
}

const THREADS: usize = 2;

struct GlobEntry {
    rel: GlobSet,
    abs: GlobSet,
}

struct IgnoreAllow {
    ignore: GlobEntry,
    allow: GlobEntry,
    have_rel: bool,
    have_abs: bool,
}

impl GlobEntry {
    fn is_match(&self, entry: &std::fs::DirEntry, cache_path: &mut Option<PathBuf>) -> bool {
        if self.rel.is_match(entry.file_name()) {
            true
        } else if !self.abs.is_empty() {
            let p = entry.path();
            let res = self.abs.is_match(&p);
            // Store expensive-to-compute path so that it may be reused.
            *cache_path = Some(p);
            res
        } else {
            false
        }
    }
}

impl IgnoreAllow {
    fn new(ignore: GlobEntry, allow: GlobEntry) -> Self {
        let have_rel = !ignore.rel.is_empty() || !allow.rel.is_empty();
        let have_abs = !ignore.abs.is_empty() || !allow.abs.is_empty();
        IgnoreAllow { ignore, allow, have_rel, have_abs }
    }

    fn is_match(&self, entry: &std::fs::DirEntry, cache_path: &mut Option<PathBuf>) -> bool {
        if self.have_rel {
            let name = entry.file_name();
            let rel_cand = globset::Candidate::new(&name);

            if self.ignore.rel.is_match_candidate(&rel_cand) {
                return false;
            }

            if self.have_abs {
                let path = entry.path();
                let abs_cand = globset::Candidate::new(&path);

                if self.ignore.abs.is_match_candidate(&abs_cand) {
                    *cache_path = Some(path);
                    return false;
                }

                let res =
                    self.allow.rel.is_match_candidate(&rel_cand) ||
                    self.allow.abs.is_match_candidate(&abs_cand);

                // Store expensive-to-compute path so that it may be reused.
                *cache_path = Some(path);

                res
            } else {
                self.allow.rel.is_match_candidate(&rel_cand)
            }
        } else if self.have_abs {
            let path = entry.path();
            let path_cand = globset::Candidate::new(&path);
            let res =
                !self.ignore.abs.is_match_candidate(&path_cand) &&
                self.allow.abs.is_match_candidate(&path_cand);
            *cache_path = Some(path);
            res
        } else {
            false
        }
    }
}

pub struct Ignores {
    files: IgnoreAllow,
    ignored_dirs: GlobEntry,
}

impl Ignores {
    pub fn new<E, I1, I2, I3, I4, S1, S2, S3, S4>(
        globs: I1,
        ignored_file_globs: I2,
        ignored_dir_globs: I3,
        ignored_dir_prefixes_globs: I4
    ) -> result::Result<Self, E>
        where
        E: From<globset::Error>,
        I1: Iterator<Item = result::Result<S1, E>>,
        I2: Iterator<Item = result::Result<S2, E>>,
        I3: Iterator<Item = result::Result<S3, E>>,
        I4: Iterator<Item = result::Result<S4, E>>,
        S1: AsRef<str>,
        S2: AsRef<str>,
        S3: AsRef<str>,
        S4: AsRef<str>,
    {
        let mut wanted_file_abs_builder = GlobSetBuilder::new();
        let mut wanted_file_rel_builder = GlobSetBuilder::new();
        let mut ignored_file_abs_builder = GlobSetBuilder::new();
        let mut ignored_file_rel_builder = GlobSetBuilder::new();
        let mut ignored_dir_abs_builder = GlobSetBuilder::new();
        let mut ignored_dir_rel_builder = GlobSetBuilder::new();

        for x in globs {
            let y = x?;
            let z = y.as_ref();
            let g = mk_glob(z)?;
            if glob_should_test_against_abs(z) {
                wanted_file_abs_builder.add(g);
            } else {
                wanted_file_rel_builder.add(g);
            }
        }
        for x in ignored_file_globs {
            let y = x?;
            let z = y.as_ref();
            let g = mk_glob(z)?;
            if glob_should_test_against_abs(z) {
                ignored_file_abs_builder.add(g);
            } else {
                ignored_file_rel_builder.add(g);
            }
        }

        {
            let mut tmp = String::new();
            for x in ignored_dir_globs {
                let y = x?;
                let z = y.as_ref();
                tmp.push_str("**/");
                tmp.extend(z.chars());
                let g = mk_glob(&tmp)?;
                if glob_should_test_against_abs(z) {
                    ignored_dir_abs_builder.add(g);
                } else {
                    ignored_dir_rel_builder.add(g);
                }
                tmp.clear();
            }
            for x in ignored_dir_prefixes_globs {
                let y = x?;
                let z = y.as_ref();
                tmp.push_str("**/");
                tmp.extend(z.chars());
                tmp.push('*');
                let g = mk_glob(&tmp)?;
                if glob_should_test_against_abs(z) {
                    ignored_dir_abs_builder.add(g);
                } else {
                    ignored_dir_rel_builder.add(g);
                }
                tmp.clear();
            }
        }

        let wanted_files_rel = wanted_file_rel_builder.build()?;
        let wanted_files_abs = wanted_file_abs_builder.build()?;
        let ignored_files_rel = ignored_file_rel_builder.build()?;
        let ignored_files_abs = ignored_file_abs_builder.build()?;
        let ignored_dirs_rel = ignored_dir_rel_builder.build()?;
        let ignored_dirs_abs = ignored_dir_abs_builder.build()?;

        Ok(Ignores {
            files: IgnoreAllow::new(
                GlobEntry { rel: ignored_files_rel, abs: ignored_files_abs },
                GlobEntry { rel: wanted_files_rel, abs: wanted_files_abs }
            ),
            ignored_dirs: GlobEntry { rel: ignored_dirs_rel, abs: ignored_dirs_abs },
        })
    }
}

#[defun]
fn find_rec<'a>(
    env: &'a Env,
    input_roots: Vector,
    input_globs: Vector,
    input_ignored_file_globs: Vector,
    input_ignored_dir_globs: Vector,
    input_ignored_dir_prefixes_globs: Vector,
) -> Result<Value<'a>>
{
    // let roots_count = input_roots.len();
    let roots = to_strings_iter(input_roots);

    let globs = to_strings_iter(input_globs);
    let ignored_file_globs = to_strings_iter(input_ignored_file_globs);
    let ignored_dir_globs = to_strings_iter(input_ignored_dir_globs);
    let ignored_dir_prefixes_globs = to_strings_iter(input_ignored_dir_prefixes_globs);

    let ignores = Ignores::new(globs, ignored_file_globs, ignored_dir_globs, ignored_dir_prefixes_globs)?;

    let mut s = StringsState::new(env)?;

    find_rec_impl(
        roots,
        &ignores,
        |x| s.update(x)
    )?;

    let (files, errs) = s.finalize()?;
    env.cons(files, errs)
}

// Define a function callable by Lisp.
pub fn find_rec_impl<'a, S, I, F, E>(
    roots: I,
    ignores: &Ignores,
    mut consume: F,
) -> result::Result<(), E>
    where
    F: FnMut(result::Result<String, String>) -> result::Result<(), E>,
    I: Iterator<Item = result::Result<S, E>> + ExactSizeIterator,
    S: AsRef<str>,
{
    let (report_result, receive_result) = mpsc::sync_channel(2 * THREADS);
    let roots_count = roots.len();

    let tasks_queue = ArrayQueue::new((10 * THREADS).max(roots_count));

    for r in roots {
        let path = std::path::PathBuf::from(std::ffi::OsString::from(r?.as_ref()));
        if !ignores.ignored_dirs.abs.is_match(&path) && !ignores.ignored_dirs.rel.is_match(&path) {
            tasks_queue.push(path).expect("Task queue should have enough size to hold initial set of roots");
        }
    }

    let tasks = &tasks_queue;

    let barr = Barrier::new(THREADS);
    let barr_ref = &barr;

    crossbeam::scope(
        move |s| -> result::Result<_, _> {

            let main_handle: ScopedJoinHandle<_> = {
                let private_report_result = report_result.clone();
                s.spawn(
                    move |_| -> result::Result<_, _> {
                        process_main(
                            barr_ref,
                            tasks,
                            private_report_result,
                            ignores,
                        )
                    }
                )
            };

            const INIT: MaybeUninit<ScopedJoinHandle<Result<()>>> = MaybeUninit::uninit();
            let mut handles: [_; THREADS] = [INIT; THREADS];
            handles[0] = MaybeUninit::new(main_handle);

            for i in 1..THREADS {
                let private_report_result = report_result.clone();
                let id: ScopedJoinHandle<_> = s.spawn(
                    move |_| -> result::Result<_, _> {
                        process_child(
                            barr_ref,
                            tasks,
                            private_report_result,
                            ignores,
                        )
                    }
                );
                handles[i] = MaybeUninit::new(id);
            }

            std::mem::drop(report_result);
            while let Ok(x) = receive_result.recv() {
               consume(x)?
            }

            for h in handles {
                unsafe {
                    h.assume_init().join().unwrap().unwrap();
                }
            }

            Ok(())
        }
    ).unwrap()
}

fn process_main(
    barr: &Barrier,
    tasks: &ArrayQueue<PathBuf>,
    report_result: mpsc::SyncSender<result::Result<String, String>>,
    ignores: &Ignores,
) -> Result<()>
{
    let mut children_awoken = false;

    let mut local_queue: Vec<PathBuf> = Vec::new();
    loop {
        let root: PathBuf = match local_queue.pop() {
            Some(x) =>
                match tasks.push(x) {
                    Ok(()) => continue,
                    Err(y) => y,
                },
            None => match tasks.pop() {
                Some(x) => x,
                None => break,
            },
        };

        process_dir(
            root,
            ignores,
            |path| match tasks.push(path) {
                Ok(()) => (),
                Err(path) => {
                    local_queue.push(path);
                }
            },
            |x| { Ok(report_result.send(x)?) },
        )?;

        if !children_awoken && tasks.is_full() {
            // Wake up children.
            barr.wait();
            children_awoken = true;
        }
    }
    Ok(())
}

fn process_child(
    barr: &Barrier,
    tasks: &ArrayQueue<PathBuf>,
    report_result: mpsc::SyncSender<result::Result<String, String>>,
    ignores: &Ignores,
) -> Result<()>
{
    barr.wait();

    let mut local_queue: Vec<PathBuf> = Vec::new();
    loop {
        let root: PathBuf = match local_queue.pop() {
            Some(x) =>
                match tasks.push(x) {
                    Ok(()) => continue,
                    Err(y) => y,
                },
            None => match tasks.pop() {
                Some(x) => x,
                None => break,
            },
        };

        process_dir(
            root,
            ignores,
            |path| match tasks.push(path) {
                Ok(()) => (),
                Err(path) => {
                    local_queue.push(path);
                }
            },
            |x| { Ok(report_result.send(x)?) },
        )?;
    }
    Ok(())
}

fn process_dir<D, F>(
    root: PathBuf,
    ignores: &Ignores,
    mut record_dir: D,
    mut record_file: F,
) -> Result<()>
    where
    D: FnMut(PathBuf) -> (),
    F: FnMut(result::Result<String, String>) -> Result<()>,
{
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let typ = entry.file_type()?;
        let mut tmp = None;
        if typ.is_dir() {
            if !ignores.ignored_dirs.is_match(&entry, &mut tmp) {
                let path = tmp.unwrap_or_else(|| entry.path());
                record_dir(path);
            }
        } else if typ.is_file() {
            if ignores.files.is_match(&entry, &mut tmp) {
                let path = tmp.unwrap_or_else(|| entry.path());
                record_file(match path.to_str() {
                    None => Err(format!("Invalid file name: {:?}", path)),
                    Some(x) => Ok(x.to_string()),
                })?
            }
        }
    }
    Ok(())
}

// Define a function callable by Lisp.
#[defun]
fn find_rec_serial<'a>(
    env: &'a Env,
    input_roots: Vector,
    input_globs: Vector,
    input_ignored_file_globs: Vector,
    input_ignored_dir_globs: Vector,
    input_ignored_dir_prefixes_globs: Vector,
) -> Result<Value<'a>>
{
    let roots = to_strings_iter(input_roots);
    let globs = to_strings_iter(input_globs);
    let ignored_file_globs = to_strings_iter(input_ignored_file_globs);
    let ignored_dir_globs = to_strings_iter(input_ignored_dir_globs);
    let ignored_dir_prefixes_globs = to_strings_iter(input_ignored_dir_prefixes_globs);

    let ignores = &Ignores::new(globs, ignored_file_globs, ignored_dir_globs, ignored_dir_prefixes_globs)?;

    let mut local_queue: Vec<PathBuf> = Vec::new();
    for r in roots {
        let path = std::path::PathBuf::from(std::ffi::OsString::from(r?));
        if !ignores.ignored_dirs.abs.is_match(&path) && !ignores.ignored_dirs.rel.is_match(&path) {
            local_queue.push(path);
        }
    }

    let mut s = StringsState::new(env)?;

    loop {
        let root: PathBuf =
            match local_queue.pop() {
                Some(x) => x,
                None => break,
            };

        process_dir(
            root,
            &ignores,
            |p| local_queue.push(p),
            |x| s.update(x),
        )?
    }
    let (files, errs) = s.finalize()?;
    env.cons(files, errs)
}

// fn extract_strings(input: Vector) -> Result<Vec<String>> {
//     input
//         .into_iter()
//         .map(String::from_lisp)
//         .collect::<Result<_>>()
// }

fn to_strings_iter<'a>(input: Vector<'a>) -> impl Iterator<Item = Result<String>> + ExactSizeIterator + 'a {
    input
        .into_iter()
        .map(String::from_lisp)
}

fn resize<'a>(env: &'a Env, v: Vector<'a>) -> Result<Vector<'a>> {
    let n = v.len();
    let res = env.make_vector(if n == 0 { 1 } else { n * 2 }, nil)?;
    for (i, x) in v.into_iter().enumerate() {
        res.set(i, x)?;
    }
    Ok(res)
}

fn take<'a>(env: &'a Env, count: usize, v: Vector<'a>) -> Result<Vector<'a>> {
    let res = env.make_vector(count, nil)?;
    for i in 0..count {
        res.set(i, v.get::<Value<'a>>(i)?)?;
    }
    Ok(res)
}

struct StringsState<'a> {
    env: &'a Env,

    a: Vector<'a>,
    cap_a: usize,
    size_a: usize,

    b: Vector<'a>,
    cap_b: usize,
    size_b: usize,
}

impl<'a> StringsState<'a> {
    fn new(env: &'a Env) -> Result<StringsState> {
        Ok(StringsState {
            env,

            a: env.make_vector(0, nil)?,
            cap_a: 0,
            size_a: 0,

            b: env.make_vector(0, nil)?,
            cap_b: 0,
            size_b: 0,
        })
    }

    fn update<A, B>(&mut self, x: result::Result<A, B>) -> Result<()>
        where
        A: IntoLisp<'a>,
        B: IntoLisp<'a>,
    {
        match x {
            Ok(y) => {
                let new_size = self.size_a + 1;
                if new_size > self.cap_a {
                    self.a = resize(self.env, self.a)?;
                    self.cap_a = self.a.len();
                }
                self.a.set(self.size_a, y.into_lisp(self.env)?)?;
                self.size_a = new_size;
            }
            Err(y) => {
                let new_size = self.size_b + 1;
                if new_size > self.cap_b {
                    self.b = resize(self.env, self.b)?;
                    self.cap_b = self.b.len();
                }
                self.b.set(self.size_b, y.into_lisp(self.env)?)?;
                self.size_b = new_size;
            }
        }
        Ok(())
    }

    fn finalize(self) -> Result<(Vector<'a>, Vector<'a>)> {
        let a = take(self.env, self.size_a, self.a)?;
        let b = take(self.env, self.size_b, self.b)?;
        Ok((a, b))
    }
}

// fn make_strings<'a, I, A, B>(env: &'a Env, items: I) -> Result<(Vector<'a>, Vector<'a>)>
//     where
//     I: IntoIterator<Item = result::Result<A, B>>,
//     A: IntoLisp<'a>,
//     B: IntoLisp<'a>,
// {
//     let mut s = StringsState::new(env)?;
//
//     for x in items {
//         s.update(x)?;
//     }
//
//     s.finalize()
// }
