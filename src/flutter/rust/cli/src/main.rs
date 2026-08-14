// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! `rustflutter` -- project tooling.
//!
//! The counterpart of `flutter create` / `flutter run`, minus the Dart. Apps
//! are GN targets under `//flutter/rust/projects`, because they link the C++
//! engine and the engine is built with GN; `create` writes the project and
//! refreshes the group that ties them into the build.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::os::raw::c_int;
use std::process::Command;

mod templates;

const PROJECTS_DIR: &str = "flutter/rust/projects";
const VERSION: &str = "0.1.0-m1";

/// Entry point, called by the C++ shim in main.cc.
///
/// The CLI is a staticlib behind a C++ shim for the same reason the apps are:
/// GN's rust_bin tool would have to own linking, and on Windows that means
/// re-deriving the MSVC linker and system library list that the C++ toolchain
/// already gets right. Arguments come from `std::env::args`, which reads the
/// real process command line, so nothing is lost by not taking argc/argv.
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_cli_main() -> c_int {
    let args: Vec<String> = env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let result = match argv.as_slice() {
        [] | ["help"] | ["--help"] | ["-h"] => {
            print_usage();
            Ok(())
        }
        ["--version"] | ["-V"] => {
            println!("rustflutter {VERSION}");
            Ok(())
        }
        ["create", name, rest @ ..] => create(name, rest),
        ["remove", name] => remove(name),
        ["list"] => list(),
        ["build", name] => build(name),
        ["run", name, rest @ ..] => run(name, rest),
        [unknown, ..] => Err(Error::Usage(format!("unknown command `{unknown}`"))),
    };

    match result {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("rustflutter: {err}");
            if let Error::Usage(_) = err {
                eprintln!();
                print_usage();
            }
            1
        }
    }
}

fn print_usage() {
    println!(
        "\
rustflutter -- a Rust UI framework on the Flutter engine

USAGE:
    rustflutter <command> [args]

COMMANDS:
    create <name> [--title <text>]   Scaffold a new app under {PROJECTS_DIR}
    remove <name>                    Delete an app and drop it from the build
    list                             List the apps in this checkout
    build <name>                     Build an app with ninja
    run <name> [-- <app args>]       Build an app and run it

    help, --version

Run from anywhere inside the checkout; the source root is located automatically."
    );
}

// -- create -------------------------------------------------------------------

fn create(name: &str, rest: &[&str]) -> Result<(), Error> {
    validate_name(name)?;

    let mut title = titlecase(name);
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "--title" => {
                title = iter
                    .next()
                    .ok_or_else(|| Error::Usage("--title needs a value".into()))?
                    .to_string();
            }
            other => return Err(Error::Usage(format!("unexpected argument `{other}`"))),
        }
    }

    let root = source_root()?;
    let project_dir = root.join(PROJECTS_DIR).join(name);
    if project_dir.exists() {
        return Err(Error::AlreadyExists(project_dir));
    }

    fs::create_dir_all(project_dir.join("src"))?;
    write(&project_dir.join("BUILD.gn"), &templates::build_gn(name))?;
    write(&project_dir.join("main.cc"), templates::MAIN_CC)?;
    write(&project_dir.join("src/main.rs"), &templates::main_rs(name, &title))?;
    write(&project_dir.join("README.md"), &templates::readme(name))?;

    refresh_projects_group(&root)?;

    let rel = project_dir
        .strip_prefix(&root)
        .unwrap_or(&project_dir)
        .display()
        .to_string()
        .replace('\\', "/");

    println!("Created `{name}` in {rel}");
    println!();
    println!("  rustflutter run {name}");
    Ok(())
}

fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::Usage("project name must not be empty".into()));
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok || name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(Error::Usage(format!(
            "`{name}` is not a valid project name: use lowercase letters, digits \
             and underscores, starting with a letter (it becomes a Rust crate \
             name and a GN target name)"
        )));
    }
    Ok(())
}

fn titlecase(name: &str) -> String {
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rewrites `projects/BUILD.gn` so every project directory is in the build.
fn refresh_projects_group(root: &Path) -> Result<(), Error> {
    let dir = root.join(PROJECTS_DIR);
    fs::create_dir_all(&dir)?;
    let mut names = project_names(root)?;
    names.sort();

    let deps = names
        .iter()
        .map(|n| format!("    \"//{PROJECTS_DIR}/{n}\",\n"))
        .collect::<String>();

    write(&dir.join("BUILD.gn"), &templates::projects_group(&deps))
}

fn project_names(root: &Path) -> Result<Vec<String>, Error> {
    let dir = root.join(PROJECTS_DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        // A directory only counts as a project once it has a BUILD.gn.
        if entry.path().join("BUILD.gn").exists() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    Ok(names)
}

/// Deletes a project and refreshes the group.
///
/// Deleting the directory by hand leaves a dangling label in
/// `projects/BUILD.gn`, which makes every subsequent `gn gen` fail -- so
/// removal has to go through the same place that maintains that file.
fn remove(name: &str) -> Result<(), Error> {
    let root = source_root()?;
    let project_dir = root.join(PROJECTS_DIR).join(name);
    if !project_dir.is_dir() {
        return Err(Error::NoSuchProject(name.to_string()));
    }

    fs::remove_dir_all(&project_dir)?;
    refresh_projects_group(&root)?;

    println!("Removed `{name}`. Re-run gn so the build graph drops the target.");
    Ok(())
}

// -- list / build / run -------------------------------------------------------

fn list() -> Result<(), Error> {
    let root = source_root()?;
    let mut names = project_names(&root)?;
    names.sort();
    if names.is_empty() {
        println!("No projects yet. Create one with `rustflutter create <name>`.");
        return Ok(());
    }
    println!("Projects in {PROJECTS_DIR}:");
    for name in names {
        println!("  {name}");
    }
    Ok(())
}

fn build(name: &str) -> Result<(), Error> {
    let root = source_root()?;
    let out_dir = out_dir(&root)?;
    let target = format!("{PROJECTS_DIR}/{name}");

    let status = Command::new("ninja")
        .arg("-C")
        .arg(&out_dir)
        .arg(&target)
        .current_dir(&root)
        .status()
        .map_err(|e| Error::Spawn("ninja".into(), e))?;

    if !status.success() {
        return Err(Error::BuildFailed(name.to_string()));
    }
    Ok(())
}

fn run(name: &str, rest: &[&str]) -> Result<(), Error> {
    build(name)?;

    let root = source_root()?;
    let out_dir = out_dir(&root)?;
    let exe = out_dir.join(format!("{name}{}", env::consts::EXE_SUFFIX));
    if !exe.exists() {
        return Err(Error::MissingExecutable(exe));
    }

    let app_args: Vec<&str> = match rest.first() {
        Some(&"--") => rest[1..].to_vec(),
        _ => rest.to_vec(),
    };

    let status = Command::new(&exe)
        .args(&app_args)
        .current_dir(&out_dir)
        .status()
        .map_err(|e| Error::Spawn(exe.display().to_string(), e))?;

    if !status.success() {
        return Err(Error::RunFailed(name.to_string()));
    }
    Ok(())
}

// -- checkout layout ----------------------------------------------------------

/// Walks up from the current directory looking for the `src` root, identified
/// by the `.gn` dotfile that marks a GN source tree.
fn source_root() -> Result<PathBuf, Error> {
    let mut dir = env::current_dir()?;
    loop {
        if dir.join(".gn").is_file() && dir.join("flutter").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(Error::NotInCheckout);
        }
    }
}

/// Picks the build directory. Prefers an explicit RUSTFLUTTER_OUT, else the
/// single directory under `out/`, else errors rather than guessing.
fn out_dir(root: &Path) -> Result<PathBuf, Error> {
    if let Ok(explicit) = env::var("RUSTFLUTTER_OUT") {
        let path = root.join("out").join(&explicit);
        return if path.is_dir() { Ok(path) } else { Err(Error::NoOutDir) };
    }

    let out = root.join("out");
    if !out.is_dir() {
        return Err(Error::NoOutDir);
    }
    let mut candidates: Vec<PathBuf> = fs::read_dir(&out)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("build.ninja").is_file())
        .map(|e| e.path())
        .collect();
    candidates.sort();

    match candidates.len() {
        0 => Err(Error::NoOutDir),
        1 => Ok(candidates.remove(0)),
        _ => {
            // More than one configuration: host_debug_unopt is the default the
            // README documents, so prefer it before giving up.
            candidates
                .iter()
                .find(|p| p.file_name().is_some_and(|n| n == "host_debug_unopt"))
                .cloned()
                .ok_or(Error::AmbiguousOutDir)
        }
    }
}

fn write(path: &Path, contents: &str) -> Result<(), Error> {
    fs::write(path, contents).map_err(Error::Io)
}

// -- errors -------------------------------------------------------------------

enum Error {
    Usage(String),
    Io(io::Error),
    NotInCheckout,
    NoOutDir,
    AmbiguousOutDir,
    AlreadyExists(PathBuf),
    Spawn(String, io::Error),
    BuildFailed(String),
    RunFailed(String),
    MissingExecutable(PathBuf),
    NoSuchProject(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Usage(msg) => write!(f, "{msg}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::NotInCheckout => write!(
                f,
                "not inside a rustflutter checkout (no `src/.gn` found above the \
                 current directory)"
            ),
            Error::NoOutDir => write!(
                f,
                "no build directory found. Run `vpython3 flutter/tools/gn \
                 --unoptimized --no-rbe` from src/ first"
            ),
            Error::AmbiguousOutDir => write!(
                f,
                "several build directories under out/. Set RUSTFLUTTER_OUT to \
                 pick one"
            ),
            Error::AlreadyExists(p) => write!(f, "{} already exists", p.display()),
            Error::Spawn(what, e) => write!(f, "could not run {what}: {e}"),
            Error::BuildFailed(n) => write!(f, "build of `{n}` failed"),
            Error::RunFailed(n) => write!(f, "`{n}` exited with a failure"),
            Error::MissingExecutable(p) => {
                write!(f, "built, but {} was not produced", p.display())
            }
            Error::NoSuchProject(n) => write!(f, "no project named `{n}`"),
        }
    }
}
