// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! `rustflutter` -- project tooling.
//!
//! The counterpart of `flutter create`, minus the Dart. An application is an
//! ordinary Cargo project that lives wherever its author wants it: `create`
//! writes one and then gets out of the way, because `cargo build` is all it
//! needs afterwards.
//!
//! It used to write GN targets inside the engine tree, since the engine is
//! built with GN and an application has to link it. That made every
//! application engine code, which it is not -- it put unrelated projects in the
//! engine's build graph and in its git history, and made every engine upgrade
//! carry them. `//flutter/rust:rustflutter_engine` fixed the underlying
//! problem by reducing the C++ side to a single archive, which a `build.rs` can
//! link without knowing GN exists.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::os::raw::c_int;
use std::process::Command;

mod templates;

/// Where the framework crate lives inside a checkout, for the path dependency.
const FRAMEWORK_DIR: &str = "flutter/rust/rustflutter";
/// The linker the engine is built with; a generated project uses the same one.
const LINKER: &str = "flutter/buildtools/windows-x64/clang/bin/lld-link.exe";
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
        ["build", rest @ ..] => cargo("build", rest),
        ["run", rest @ ..] => cargo("run", rest),
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
    create <name> [--title <text>] [--path <dir>]
                                     Scaffold a Cargo project. Written to
                                     <dir>/<name>, defaulting to ./<name>.
    build [-- <cargo args>]          `cargo build` in the current project
    run   [-- <app args>]            `cargo run` in the current project

    help, --version

`create` must be run from inside a checkout, so it can find the framework and
the engine build to point the new project at. Nothing else has to be: a project
is a Cargo project and `cargo build` works on it directly."
    );
}

// -- create -------------------------------------------------------------------

fn create(name: &str, rest: &[&str]) -> Result<(), Error> {
    validate_name(name)?;

    let mut title = titlecase(name);
    let mut where_to: Option<PathBuf> = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "--title" => {
                title = iter
                    .next()
                    .ok_or_else(|| Error::Usage("--title needs a value".into()))?
                    .to_string();
            }
            "--path" => {
                let value = iter
                    .next()
                    .ok_or_else(|| Error::Usage("--path needs a value".into()))?;
                where_to = Some(PathBuf::from(value));
            }
            other => return Err(Error::Usage(format!("unexpected argument `{other}`"))),
        }
    }

    // The checkout is needed to write the project, not to build it: it is where
    // the framework crate and the engine archive are.
    let root = source_root()?;
    let engine_out = release_out_dir(&root)?;

    let parent = match where_to {
        Some(path) => path,
        None => env::current_dir()?,
    };
    let project_dir = parent.join(name);
    if project_dir.exists() {
        return Err(Error::AlreadyExists(project_dir));
    }

    let framework = forward_slashes(&root.join(FRAMEWORK_DIR));
    let linker = forward_slashes(&root.join(LINKER));
    let engine = forward_slashes(&engine_out);

    fs::create_dir_all(project_dir.join("src"))?;
    fs::create_dir_all(project_dir.join(".cargo"))?;
    write(&project_dir.join("Cargo.toml"), &templates::cargo_toml(name, &framework))?;
    write(&project_dir.join(".cargo/config.toml"), &templates::cargo_config(&linker))?;
    write(&project_dir.join("build.rs"), &templates::build_rs(&engine))?;
    write(&project_dir.join("src/main.rs"), &templates::main_rs(name, &title))?;
    write(&project_dir.join(".gitignore"), templates::gitignore())?;
    write(&project_dir.join("README.md"), &templates::readme(name, &engine))?;

    println!("Created `{name}` in {}", project_dir.display());
    if !engine_out.join("obj/flutter/rust/rustflutter_engine.lib").exists() {
        println!();
        println!("The engine archive is not built yet. From {}:", root.display());
        println!("  ninja -C {} flutter/rust:rustflutter_engine", relative_out(&root, &engine_out));
    }
    println!();
    println!("  cd {}", project_dir.display());
    println!("  cargo run");
    Ok(())
}

/// Paths go into generated files, which are read by humans and by Cargo. Both
/// prefer forward slashes on Windows, and TOML would treat a backslash as an
/// escape.
fn forward_slashes(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn relative_out(root: &Path, out: &Path) -> String {
    forward_slashes(out.strip_prefix(root).unwrap_or(out))
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
             name)"
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

// -- build / run --------------------------------------------------------------

/// Hands the command to Cargo in the current directory.
///
/// A thin pass-through, and deliberately so: a generated project is a Cargo
/// project, so `cargo build` already works and this only saves remembering
/// that. Anything Cargo can do that this cannot, do with Cargo.
fn cargo(subcommand: &str, rest: &[&str]) -> Result<(), Error> {
    if !Path::new("Cargo.toml").is_file() {
        return Err(Error::NotAProject);
    }
    let passthrough: Vec<&str> = match rest.first() {
        Some(&"--") => rest.to_vec(),
        _ if rest.is_empty() => Vec::new(),
        _ => rest.to_vec(),
    };

    let status = Command::new("cargo")
        .arg(subcommand)
        .args(&passthrough)
        .status()
        .map_err(|e| Error::Spawn("cargo".into(), e))?;

    if !status.success() {
        return Err(Error::CargoFailed(subcommand.to_string()));
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

/// Picks the engine build a generated project should link against.
///
/// A release one, and not as a preference. rustc links the static CRT, the
/// release engine is built `/MT` and a debug engine `/MTd`, and mixing the two
/// fails at the very end of the link with every CRT symbol defined twice. So a
/// debug build directory is not a worse answer here, it is a broken one.
///
/// RUSTFLUTTER_OUT overrides, for a checkout whose release build is named
/// something else. It is taken at its word -- including the `/MT` part.
fn release_out_dir(root: &Path) -> Result<PathBuf, Error> {
    if let Ok(explicit) = env::var("RUSTFLUTTER_OUT") {
        let path = root.join("out").join(&explicit);
        return if path.is_dir() { Ok(path) } else { Err(Error::NoOutDir) };
    }

    let out = root.join("out");
    if !out.is_dir() {
        return Err(Error::NoReleaseOutDir);
    }
    let candidates: Vec<PathBuf> = fs::read_dir(&out)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("build.ninja").is_file())
        .map(|e| e.path())
        .collect();

    candidates
        .iter()
        .find(|p| p.file_name().is_some_and(|n| n == "host_release"))
        .or_else(|| {
            candidates.iter().find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.contains("release"))
            })
        })
        .cloned()
        .ok_or(Error::NoReleaseOutDir)
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
    NoReleaseOutDir,
    NotAProject,
    AlreadyExists(PathBuf),
    Spawn(String, io::Error),
    CargoFailed(String),
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
                "RUSTFLUTTER_OUT names a directory that is not under out/"
            ),
            Error::NoReleaseOutDir => write!(
                f,
                "no release engine build found under out/. A project links the \
                 static CRT, so it needs one:\n  vpython3 flutter/tools/gn \
                 --runtime-mode=release --no-rbe\n  ninja -C out/host_release \
                 flutter/rust:rustflutter_engine"
            ),
            Error::NotAProject => write!(
                f,
                "no Cargo.toml here. Run this from inside a project, or create \
                 one with `rustflutter create <name>`"
            ),
            Error::AlreadyExists(p) => write!(f, "{} already exists", p.display()),
            Error::Spawn(what, e) => write!(f, "could not run {what}: {e}"),
            Error::CargoFailed(c) => write!(f, "`cargo {c}` failed"),
        }
    }
}
