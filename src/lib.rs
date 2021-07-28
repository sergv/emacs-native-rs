
#![allow(unused_mut)]

use std::iter::IntoIterator;
use std::path::{PathBuf, Path};
use std::result;
use std::sync::{Arc, mpsc};

use anyhow;
use emacs;
use emacs::{defun, Env, Result, Value, Vector, FromLisp, IntoLisp};
use grep::matcher::Matcher;
use grep::regex::{RegexMatcher, RegexMatcherBuilder};
use grep::searcher::{self, Searcher, SearcherBuilder};
use pathdiff;

pub mod find;
pub mod fuzzy_match;

emacs::use_symbols!(nil make_egrep_match);

// Emacs won't load the module without this.
emacs::plugin_is_GPL_compatible!();

// Register the initialization hook that Emacs will call when it loads the module.
#[emacs::module(name = "emacs_native")]
fn init(env: &Env) -> Result<Value<'_>> {
    ().into_lisp(env)
}

#[defun]
fn binary_search<'a>(
    needle: i64,
    haystack: Vector,
) -> Result<bool>
{
    let mut start = 0;
    let mut end = haystack.len();
    let mut mid = end / 2;
    while start != end {
        let x: i64 = i64::from_lisp(haystack.get(mid)?)?;
        if x == needle {
            return Ok(true);
        }
        if x < needle {
            start = mid + 1;
        } else {
            end = mid;
        }
        mid = (start + end) / 2;
    }
    return Ok(false)
}

#[defun]
fn find_rec<'a>(
    env: &'a Env,
    input_roots: Vector,
    input_globs: Vector,
    input_ignored_file_globs: Vector,
    input_ignored_dir_globs: Vector,
    input_ignored_dir_prefixes_globs: Vector,
    input_ignored_abs_dirs: Vector,
) -> Result<Value<'a>>
{
    // let roots_count = input_roots.len();
    let roots = to_strings_iter(input_roots);

    let globs = to_strings_iter(input_globs);
    let ignored_file_globs = to_strings_iter(input_ignored_file_globs);
    let ignored_dir_globs = to_strings_iter(input_ignored_dir_globs);
    let ignored_dir_prefixes_globs = to_strings_iter(input_ignored_dir_prefixes_globs);
    let ignored_abs_dirs = to_strings_iter(input_ignored_abs_dirs);

    let ignores = find::Ignores::new(globs, ignored_file_globs, ignored_dir_globs, ignored_dir_prefixes_globs, ignored_abs_dirs)?;

    let mut s = SuccessesAndErrors::new(env)?;

    find::find_rec(
        roots,
        &ignores,
        || Ok(()),
        |_state, _orig_root: (), x, chan| {
            chan.send(path_to_string(x)).map_err(anyhow::Error::new)
        },
        |y| s.update(y)
    )?;

    let (files, errs) = s.finalize()?;
    env.cons(files, errs)
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
    input_ignored_abs_dirs: Vector,
) -> Result<Value<'a>>
{
    let roots = to_strings_iter(input_roots);
    let globs = to_strings_iter(input_globs);
    let ignored_file_globs = to_strings_iter(input_ignored_file_globs);
    let ignored_dir_globs = to_strings_iter(input_ignored_dir_globs);
    let ignored_dir_prefixes_globs = to_strings_iter(input_ignored_dir_prefixes_globs);
    let ignored_abs_dirs = to_strings_iter(input_ignored_abs_dirs);

    let ignores = &find::Ignores::new(globs, ignored_file_globs, ignored_dir_globs, ignored_dir_prefixes_globs, ignored_abs_dirs)?;

    let mut local_queue: Vec<PathBuf> = Vec::new();
    for r in roots {
        let path = std::path::PathBuf::from(std::ffi::OsString::from(r?));
        if !ignores.ignored_dirs.abs.is_match(&path) && !ignores.ignored_dirs.rel.is_match(&path) {
            local_queue.push(path);
        }
    }

    let mut s = SuccessesAndErrors::new(env)?;

    loop {
        let root: PathBuf =
            match local_queue.pop() {
                Some(x) => x,
                None => break,
            };

        find::visit_dir(
            root,
            &ignores,
            |p| local_queue.push(p),
            |x| s.update(path_to_string(x)),
        )?
    }
    let (files, errs) = s.finalize()?;
    env.cons(files, errs)
}

#[defun]
fn grep<'a>(
    env: &'a Env,
    input_roots: Vector,
    regexp: String,
    input_globs: Vector,
    input_ignored_file_globs: Vector,
    input_ignored_dir_globs: Vector,
    input_ignored_dir_prefixes_globs: Vector,
    input_ignored_abs_dirs: Vector,
    input_case_insensitive: Value,
) -> Result<Value<'a>>
{
    // let roots_count = input_roots.len();
    let roots = to_strings_iter(input_roots);

    let globs = to_strings_iter(input_globs);
    let ignored_file_globs = to_strings_iter(input_ignored_file_globs);
    let ignored_dir_globs = to_strings_iter(input_ignored_dir_globs);
    let ignored_dir_prefixes_globs = to_strings_iter(input_ignored_dir_prefixes_globs);
    let ignored_abs_dirs = to_strings_iter(input_ignored_abs_dirs);

    let case_insensitive = !nil.bind(env).eq(input_case_insensitive);

    let ignores = find::Ignores::new(globs, ignored_file_globs, ignored_dir_globs, ignored_dir_prefixes_globs, ignored_abs_dirs)?;

    let mut results = Successes::new(env)?;

    find::find_rec(
        roots,
        &ignores,
        || {
            let searcher = SearcherBuilder::new()
                .line_number(true)
                .multi_line(true)
                .memory_map(unsafe { grep::searcher::MmapChoice::auto() })
                .binary_detection(grep::searcher::BinaryDetection::none())
                .build();

            let matcher = RegexMatcherBuilder::new()
                .case_insensitive(case_insensitive)
                .multi_line(true)
                .build(&regexp)?;

            Ok((searcher, matcher))
        },
        |(searcher, matcher), orig_root: Arc<PathBuf>, path, results| {
            let sink = GrepSink {
                rel_path_cache: None,
                abs_path_cache: None,
                abs_path: &path,
                orig_root: &*orig_root,
                matcher: &matcher,
                results: &results,
            };
            searcher.search_path(&*matcher, &path, sink).map_err(|x| x.err)
        },
        |m| {
            results.update(m)
        }
    )?;

    results.finalize()?.into_lisp(env)
}

// fn extract_strings(input: Vector) -> Result<Vec<String>> {
//     input
//         .into_iter()
//         .map(String::from_lisp)
//         .collect::<Result<_>>()
// }

struct EmacsPath {
    path: String,
}

impl EmacsPath {
    fn new(path: PathBuf) -> result::Result<Self, Error> {
        match path.into_os_string().into_string() {
            Err(err) => Err(Error::msg(format!("Path has invalid utf8 encoding: {:?}", err))),
            Ok(s) => {
                #[cfg(target_family = "windows")]
                let path = unsafe {
                    for b in s.as_bytes_mut() {
                        match b {
                            b'\\' => *b = b'/',
                            _ => (),
                        }
                    }
                    s
                };
                #[cfg(target_family = "unix")]
                let path = s;
                Ok(EmacsPath { path })
            }
        }
    }
}

impl<'a> emacs::IntoLisp<'a> for &EmacsPath {
    fn into_lisp(self, env: &'a Env) -> Result<Value<'a>> {
        self.path.clone().into_lisp(env)
    }
}

struct Match {
    line: u64,
    byte_offset: u64,
    prefix: String,
    body: String,
    suffix: String,
    abs_path: Arc<EmacsPath>,
    rel_path: Arc<EmacsPath>,
}

impl<'a> emacs::IntoLisp<'a> for Match {
    fn into_lisp(self, env: &'a Env) -> Result<Value<'a>> {
        env.call(
            make_egrep_match,
            (&*self.abs_path,
             &*self.rel_path,
             self.line,
             self.byte_offset,
             self.prefix,
             self.body,
             self.suffix
            )
        )
    }
}

struct GrepSink<'a, 'b, 'c> {
    rel_path_cache: Option<Arc<EmacsPath>>,
    abs_path_cache: Option<Arc<EmacsPath>>,
    abs_path: &'a Path,
    orig_root: &'a Path,
    matcher: &'b RegexMatcher,
    results: &'c mpsc::SyncSender<Match>,
}

struct Error {
    err: emacs::Error,
}

impl Error {
    fn msg<T>(msg: T) -> Self
        where
        T: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static
    {
        Error {
            err: emacs::Error::msg(msg)
        }
    }
}

impl searcher::SinkError for Error {
    fn error_message<T: std::fmt::Display>(message: T) -> Self {
        Error::msg(message.to_string())
    }
}

impl<'a, 'b, 'c> searcher::Sink for GrepSink<'a, 'b, 'c> {
    type Error = Error;

    fn matched(&mut self, _searcher: &Searcher, m: &searcher::SinkMatch) -> result::Result<bool, Self::Error> {
        let line = m.line_number().expect("Line numbers must be available");
        let byte_offset = m.absolute_byte_offset();
        let matched_lines = m.bytes();

        let rel_path = match self.rel_path_cache {
            Some(ref x) => x,
            None => {
                let path = match pathdiff::diff_paths(self.abs_path, self.orig_root) {
                    None => return Err(Error::msg(format!(
                        "Invariant violation: abs_path {:?} should be under orig_root {:?}",
                        self.abs_path,
                        self.orig_root
                    ))),
                    Some(x) => x,
                };
                self.rel_path_cache = Some(Arc::new(EmacsPath::new(path)?));
                self.rel_path_cache.as_ref().unwrap()
            }
        };

        let abs_path = match self.abs_path_cache {
            Some(ref x) => x,
            None => {
                self.abs_path_cache = Some(Arc::new(EmacsPath::new(self.abs_path.to_owned())?));
                self.abs_path_cache.as_ref().unwrap()
            }
        };

        let submatch = self.matcher.find(matched_lines).unwrap().expect("Match is guaranteed to be found");

        let prefix = std::str::from_utf8(&matched_lines[..submatch.start()])
            .map_err(|err| Error::msg(format!("Failed to utf-8 encode match prefix: {}", err)))?;
        let body = std::str::from_utf8(&matched_lines[submatch])
            .map_err(|err| Error::msg(format!("Failed to utf-8 encode match body: {}", err)))?;
        let suffix = std::str::from_utf8(&matched_lines[submatch.end()..])
            .map_err(|err| Error::msg(format!("Failed to utf-8 encode match suffix: {}", err)))?
            .trim_end_matches(|c| c == '\r' || c == '\n');

        self.results.send(Match {
            line,
            byte_offset,
            prefix: prefix.to_string(),
            body: body.to_string(),
            suffix: suffix.to_string(),
            rel_path: rel_path.clone(),
            abs_path: abs_path.clone(),
        }).map_err(|err| Error::msg(format!("Failed to send match: {}", err)))?;

        // println!(
        //     "{}:{}@{}@@{:?}:{:?}",
        //     rel_path.display(),
        //     line,
        //     offset,
        //     m.bytes_range_in_buffer(),
        //     submatch
        //     // std::str::from_utf8(text)
        // );

        // let len = m.

            // env.call_unprotected("", args)

        Ok(true)
    }
}

fn path_to_string(path: PathBuf) -> result::Result<String, String> {
    match path.to_str() {
        None => Err(format!("Invalid file name: {:?}", path)),
        Some(x) => Ok(x.to_string()),
    }
}

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

struct SuccessesAndErrors<'a> {
    env: &'a Env,

    a: Vector<'a>,
    cap_a: usize,
    size_a: usize,

    b: Vector<'a>,
    cap_b: usize,
    size_b: usize,
}

impl<'a> SuccessesAndErrors<'a> {
    fn new(env: &'a Env) -> Result<SuccessesAndErrors> {
        Ok(SuccessesAndErrors {
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

struct Successes<'a> {
    env: &'a Env,

    items: Vector<'a>,
    cap: usize,
    size: usize,
}

impl<'a> Successes<'a> {
    fn new(env: &'a Env) -> Result<Successes> {
        Ok(Successes {
            env,
            items: env.make_vector(0, nil)?,
            cap: 0,
            size: 0,
        })
    }

    fn update<A>(&mut self, x: A) -> Result<()>
        where
        A: IntoLisp<'a>,
    {
        let new_size = self.size + 1;
        if new_size > self.cap {
            self.items = resize(self.env, self.items)?;
            self.cap = self.items.len();
        }
        self.items.set(self.size, x.into_lisp(self.env)?)?;
        self.size = new_size;
        Ok(())
    }

    fn finalize(self) -> Result<Vector<'a>> {
        let a = take(self.env, self.size, self.items)?;
        Ok(a)
    }
}
