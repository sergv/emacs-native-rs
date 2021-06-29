// #[cfg(test)]
// mod tests {
//     #[test]
//     fn it_works() {
//         assert_eq!(2 + 2, 4);
//     }
// }

use std::iter::IntoIterator;

use ignore::WalkBuilder;
// use ignore::WalkState;
use ignore::overrides::OverrideBuilder;

use emacs::{defun, Env, Result, Value, Vector, FromLisp, IntoLisp};


// use_symbols!(nil);

// Emacs won't load the module without this.
emacs::plugin_is_GPL_compatible!();

// Register the initialization hook that Emacs will call when it loads the module.
#[emacs::module(name = "emacs_native")]
fn init(env: &Env) -> Result<Value<'_>> {
    ().into_lisp(env)
}

// Define a function callable by Lisp.
#[defun]
fn find_rec<'a>(
    env: &'a Env,
    current_dir: String,
    input_roots: Vector,
    input_globs: Vector,
    input_ignored_file_globs: Vector,
    input_ignored_dir_globs: Vector,
    input_ignored_dir_prefixes_globs: Vector
) -> Result<Value<'a>>
{
    let mut roots = to_strings_iter(input_roots);
    let globs = to_strings_iter(input_globs);
    let ignored_file_globs = to_strings_iter(input_ignored_file_globs);
    let ignored_dir_globs = to_strings_iter(input_ignored_dir_globs);
    let ignored_dir_prefixes_globs = to_strings_iter(input_ignored_dir_prefixes_globs);

    let root = match roots.next() {
        Some(x) => x?,
        None => return ().into_lisp(env),
    };

    let mut b = WalkBuilder::new(root);
    for root in roots {
        b.add(root?);
    }
    b.follow_links(true);
    b.hidden(false);

    let mut ovb = OverrideBuilder::new(current_dir);
    ovb.case_insensitive(true)?;
    for x in globs {
        ovb.add(&x?)?;
    }
    let mut tmp = String::new();
    for x in ignored_file_globs {
        tmp.push('!');
        tmp.extend(x?.chars());
        ovb.add(&tmp)?;
        tmp.clear();
    }
    for x in ignored_dir_globs {
        tmp.push_str("!**/");
        tmp.extend(x?.chars());
        ovb.add(&tmp)?;
        tmp.clear();
    }
    for x in ignored_dir_prefixes_globs {
        tmp.push_str("!**/");
        tmp.extend(x?.chars());
        tmp.push('*');
        ovb.add(&tmp)?;
        tmp.clear();
    }
    let ov = ovb.build()?;

    b.overrides(ov);

    let (files, errs) = make_strings(
        env,
        b.build().filter_map(
            |x|
            match x {
                Err(err) => Some(Err(err.to_string())),
                Ok(entry) => {
                    match entry.file_type() {
                        Some(typ) if typ.is_file() =>
                            match entry.path().to_str() {
                                Some(y) => Some(Ok(y.to_string())),
                                None => Some(Err(format!("Invalid utf-8 path: {}", entry.into_path().display())))
                            },
                        _ => None,
                    }
                }
            }
        ))?;

    env.cons(files, errs)
}

// Define a function callable by Lisp.
#[defun]
fn find_rec_opt<'a>(
    env: &'a Env,
    current_dir: String,
    input_roots: Vector,
    input_globs: Vector,
    input_ignored_file_globs: Vector,
    input_ignored_dir_globs: Vector,
    input_ignored_dir_prefixes_globs: Vector
) -> Result<Value<'a>>
{
    let mut roots = to_strings_iter(input_roots);
    let globs = to_strings_iter(input_globs);
    let ignored_file_globs = to_strings_iter(input_ignored_file_globs);
    let ignored_dir_globs = to_strings_iter(input_ignored_dir_globs);
    let ignored_dir_prefixes_globs = to_strings_iter(input_ignored_dir_prefixes_globs);

    let root = match roots.next() {
        Some(x) => x?,
        None => return ().into_lisp(env),
    };

    let mut b = WalkBuilder::new(root);
    for root in roots {
        b.add(root?);
    }
    b.follow_links(true);
    b.hidden(false);

    let mut ovb = OverrideBuilder::new(current_dir);
    ovb.case_insensitive(true)?;
    for x in globs {
        ovb.add(&x?)?;
    }
    let mut tmp = String::new();
    for x in ignored_file_globs {
        tmp.push('!');
        tmp.extend(x?.chars());
        ovb.add(&tmp)?;
        tmp.clear();
    }
    for x in ignored_dir_globs {
        tmp.push_str("!**/");
        tmp.extend(x?.chars());
        ovb.add(&tmp)?;
        tmp.clear();
    }
    for x in ignored_dir_prefixes_globs {
        tmp.push_str("!**/");
        tmp.extend(x?.chars());
        tmp.push('*');
        ovb.add(&tmp)?;
        tmp.clear();
    }
    let ov = ovb.build()?;

    b.overrides(ov);

    // let (files, errs) = make_strings(
    //     env,
    //     b.build().filter_map(
    //         |x|
    //         match x {
    //             Err(err) => Some(Err(err.to_string())),
    //             Ok(entry) => {
    //                 match entry.file_type() {
    //                     Some(typ) if typ.is_file() =>
    //                         match entry.path().to_str() {
    //                             Some(y) => Some(Ok(y.to_string())),
    //                             None => Some(Err(format!("Invalid utf-8 path: {}", entry.into_path().display())))
    //                         },
    //                     _ => None,
    //                 }
    //             }
    //         }
    //     ))?;

    // env.message(&format!("Hello, {}, {}!", , String::from_lisp(env.call("format", ("%s", y,))?)?))

    // let e = &mut errs;
    // let res = &mut results;

    b.build_parallel().run(
        || Box::new(
            |path| match path {
                Err(err) => {
                    // e.push(format!("Error: {}", err));
                    WalkState::Continue
                }
                Ok(entry) => {
                    res.push(entry.into_path());
                    WalkState::Continue
                }
            })
    );

    // env.cons(files, errs)

    ().into_lisp(env)
}

fn extract_strings(input: Vector) -> Result<Vec<String>> {
    input
        .into_iter()
        .map(String::from_lisp)
        .collect::<Result<_>>()
}

fn to_strings_iter<'a>(input: Vector<'a>) -> impl Iterator<Item = Result<String>> + 'a {
    input
        .into_iter()
        .map(String::from_lisp)
}

fn resize<'a>(env: &'a Env, nil: Value<'a>, v: Vector<'a>) -> Result<Vector<'a>> {
    let n = v.len();
    let res = env.make_vector(if n == 0 { 1 } else { n * 2 }, nil)?;
    for (i, x) in v.into_iter().enumerate() {
        res.set(i, x)?;
    }
    Ok(res)
}

fn take<'a>(env: &'a Env, nil: Value<'a>, count: usize, v: Vector<'a>) -> Result<Vector<'a>> {
    let res = env.make_vector(count, nil)?;
    for i in 0..count {
        res.set(i, v.get::<Value<'a>>(i)?)?;
    }
    Ok(res)
}

fn make_strings<'a, I, A, B>(env: &'a Env, items: I) -> Result<(Vector<'a>, Vector<'a>)>
    where
    I: IntoIterator<Item = std::result::Result<A, B>>,
    A: IntoLisp<'a>,
    B: IntoLisp<'a>,
{
    let nil = ().into_lisp(env)?;
    let mut a = env.make_vector(0, nil)?;
    let mut cap_a = 0;
    let mut size_a = 0;

    let mut b = env.make_vector(0, nil)?;
    let mut cap_b = 0;
    let mut size_b = 0;

    for x in items {
        match x {
            Ok(y) => {
                let new_size = size_a + 1;
                if new_size > cap_a {
                    a = resize(env, nil, a)?;
                    cap_a = a.len();
                }
                a.set(size_a, y.into_lisp(env)?)?;
                size_a = new_size;
            }
            Err(y) => {
                let new_size = size_b + 1;
                if new_size > cap_b {
                    a = resize(env, nil, b)?;
                    cap_b = b.len();
                }
                b.set(size_b, y.into_lisp(env)?)?;
                size_b = new_size;

            }
        }
    }

    Ok((take(env, nil, size_a, a)?, take(env, nil, size_b, b)?))

    // let res = env.cons(nil, nil)?;
    // let mut curr = res;
    // for x in i {
    //     let y = x.into_lisp(env)?;
    //     let tmp = env.cons(y, nil)?;
    //     env.call("setcdr", (curr, tmp))?;
    //     curr = tmp;
    // }
    // res.cdr()
}
