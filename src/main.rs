#![forbid(unsafe_code)]

//! gpenv: load environment variables from ONE gopass entry, with one decrypt.
//!
//!   gpenv load ENTRY [--shell sh|bash|zsh|nu]   print exports for the shell to evaluate
//!   gpenv unload [--shell sh|bash|zsh|nu]       print commands that remove them again
//!   gpenv names ENTRY                           list the variable names (never values)
//!
//! gpenv never changes your shell itself (no program can). Your shell
//! evaluates its output; see README.md for the bash/zsh/nushell wrappers.
//!
//! On any error, gpenv prints nothing to stdout and exits 1, so a wrapper like
//! `out=$(gpenv load …) && eval "$out"` loads nothing rather than half.

use std::{
    io::Write,
    process::{Command, ExitCode, Stdio},
};

use gpenv::{LOADED_VAR, Shell, merge_loaded, parse, render_load, render_unload};

const USAGE: &str = "\
usage:
  gpenv load ENTRY [--shell sh|bash|zsh|nu]   print exports for your shell to evaluate
  gpenv unload [--shell sh|bash|zsh|nu]       print commands that remove what was loaded
  gpenv names ENTRY                           list the variable names in ENTRY (no values)

ENTRY is a gopass entry holding NAME=value lines. Set GPENV_GOPASS to use a
different gopass binary. Shell wrappers: see README.md.";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(out) => {
            let mut stdout = std::io::stdout().lock();
            if stdout
                .write_all(out.as_bytes())
                .and_then(|()| stdout.flush())
                .is_err()
            {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gpenv: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<String, String> {
    let mut positional = Vec::new();
    let mut shell = Shell::Sh;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Ok(format!("{USAGE}\n")),
            "-V" | "--version" => return Ok(format!("gpenv {}\n", env!("CARGO_PKG_VERSION"))),
            "--shell" | "-s" => {
                shell = it.next().ok_or("--shell needs a value")?.parse()?;
            }
            s if s.starts_with("--shell=") => shell = s["--shell=".len()..].parse()?,
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n{USAGE}")),
            _ => positional.push(a),
        }
    }
    let loaded = std::env::var(LOADED_VAR).ok();
    match positional.as_slice() {
        [cmd, entry] if cmd == "load" => {
            let vars = read_entry(entry)?;
            Ok(render_load(
                &vars,
                &merge_loaded(loaded.as_deref(), &vars),
                shell,
            ))
        }
        [cmd, entry] if cmd == "names" => {
            let vars = read_entry(entry)?;
            Ok(vars.iter().map(|v| format!("{}\n", v.name)).collect())
        }
        [cmd] if cmd == "unload" => Ok(render_unload(loaded.as_deref(), shell)),
        _ => Err(USAGE.into()),
    }
}

/// Runs `gopass show ENTRY` once. stdin and stderr stay attached to the
/// terminal, so a pinentry prompt (or gopass's own errors) work as usual.
fn read_entry(entry: &str) -> Result<Vec<gpenv::Var>, String> {
    if entry.starts_with('-') {
        return Err(format!("{entry:?} is not an entry name"));
    }
    let gopass = std::env::var("GPENV_GOPASS").unwrap_or_else(|_| "gopass".into());
    let out = Command::new(&gopass)
        .args(["show", "--", entry])
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("running {gopass}: {e}"))?;
    if !out.status.success() {
        return Err(format!("`{gopass} show {entry}` failed ({})", out.status));
    }
    let mut stdout = out.stdout;
    let parsed = match std::str::from_utf8(&stdout) {
        Ok(text) => parse(text).map_err(|e| format!("{entry}: {e}")),
        Err(_) => Err(format!("{entry}: not valid UTF-8")),
    };
    // Best effort: don't leave the decrypted entry lying in freed memory.
    stdout.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(&stdout);
    parsed
}
