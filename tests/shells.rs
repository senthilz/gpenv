//! End to end: the real gpenv binary, a stand-in gopass (a shell script),
//! and the real bash / zsh / nu. Each shell loads the entry through the
//! README wrapper, prints every value back, and must get exactly the
//! original bytes, with no command inside a value executed.
//!
//! A shell that isn't installed is skipped with a note, not failed.

#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

const ENTRY: &str = r#"# test entry
PLAIN=hello
SPACES=two words  and more
QUOTE=it's "quoted"
DOLLAR=$HOME ${PATH} $(touch CANARY) `touch CANARY`
BACKSLASH=a\nb\\c
SEMI=x; touch CANARY; echo
GITHUB_TOKEN,GH_TOKEN=shared-value
UNICODE=ünïcødé ✓
EMPTY=
"#;

/// (name, expected value). CANARY is replaced with the real canary path.
fn expected() -> Vec<(&'static str, String)> {
    vec![
        ("PLAIN", "hello".into()),
        ("SPACES", "two words  and more".into()),
        ("QUOTE", "it's \"quoted\"".into()),
        (
            "DOLLAR",
            "$HOME ${PATH} $(touch CANARY) `touch CANARY`".into(),
        ),
        ("BACKSLASH", "a\\nb\\\\c".into()),
        ("SEMI", "x; touch CANARY; echo".into()),
        ("GITHUB_TOKEN", "shared-value".into()),
        ("GH_TOKEN", "shared-value".into()),
        ("UNICODE", "ünïcødé ✓".into()),
        ("EMPTY", String::new()),
    ]
}

struct Fixture {
    dir: tempdir::Dir,
    canary: String,
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    pub fn new(tag: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("gpenv-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

fn fixture(tag: &str) -> Fixture {
    let dir = tempdir::new(tag);
    let canary = dir.0.join("CANARY").display().to_string();
    let entry = ENTRY.replace("CANARY", &canary);
    std::fs::write(dir.0.join("entry.txt"), entry).unwrap();
    // Stand-in gopass: `gopass show -- NAME` prints the entry; anything else fails.
    let stub = format!(
        "#!/bin/sh\n[ \"$1\" = show ] && [ \"$2\" = -- ] && [ \"$3\" = test/entry ] && exec cat '{}'\necho \"stub gopass: bad args: $*\" >&2; exit 1\n",
        dir.0.join("entry.txt").display()
    );
    let stub_path = dir.0.join("gopass");
    std::fs::write(&stub_path, stub).unwrap();
    std::fs::set_permissions(&stub_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    Fixture { dir, canary }
}

fn have(shell: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {shell}"))
        .output()
        .is_ok_and(|o| o.status.success())
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_gpenv")
}

/// Runs `script` in `shell` with gpenv and the stub gopass on PATH.
fn run(shell: &str, args: &[&str], script: &str, fx: &Fixture) -> std::process::Output {
    let bin_dir = Path::new(bin()).parent().unwrap().display().to_string();
    let path = format!(
        "{}:{bin_dir}:{}",
        fx.dir.0.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(shell)
        .args(args)
        .arg(script)
        .env_clear()
        .env("PATH", path)
        .env("HOME", &fx.dir.0)
        .output()
        .unwrap()
}

fn check_values(shell: &str, out: &std::process::Output, fx: &Fixture) {
    assert!(
        out.status.success(),
        "{shell} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    // Each value is printed as NAME<US>value<RS>, so newlines in values are safe.
    let got: Vec<(String, String)> = stdout
        .split('\x1e')
        .filter(|r| !r.is_empty() && *r != "\n")
        .map(|r| {
            let (k, v) = r.split_once('\x1f').unwrap_or((r, ""));
            (k.trim_start_matches('\n').to_owned(), v.to_owned())
        })
        .collect();
    for (name, want) in expected() {
        let want = want.replace("CANARY", &fx.canary);
        let found = got.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        assert_eq!(found, Some(want.as_str()), "{shell}: value of {name}");
    }
    assert!(
        !Path::new(&fx.canary).exists(),
        "{shell}: a value was executed"
    );
}

fn names_list() -> String {
    expected()
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn bash_and_zsh_round_trip_and_unload() {
    for shell in ["bash", "zsh"] {
        if !have(shell) {
            eprintln!("skipping {shell}: not installed");
            continue;
        }
        let fx = fixture(shell);
        let script = format!(
            r#"
gpenv-load()   {{ local out; out=$(command gpenv load "$1" --shell sh) && eval "$out"; }}
gpenv-unload() {{ local out; out=$(command gpenv unload --shell sh) && eval "$out"; }}
gpenv-load test/entry || exit 9
for n in {names}; do eval "v=\${{$n}}"; printf '%s\037%s\036' "$n" "$v"; done
# exported to children, not just set:
sh -c 'printf "%s\037%s\036" CHILD_SEES "$GH_TOKEN"'
gpenv-unload
[ -z "${{PLAIN+x}}${{GH_TOKEN+x}}${{GPENV_LOADED+x}}" ] || {{ echo "unload left variables" >&2; exit 8; }}
"#,
            names = names_list()
        );
        let out = run(shell, &["-c"], &script, &fx);
        check_values(shell, &out, &fx);
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("CHILD_SEES\x1fshared-value"),
            "{shell}: not exported"
        );
    }
}

#[test]
fn nushell_round_trip_and_unload() {
    if !have("nu") {
        eprintln!("skipping nu: not installed");
        return;
    }
    let fx = fixture("nu");
    let script = format!(
        r#"
def --env gpenv-load [entry: string] {{ gpenv load $entry --shell nu | from json | load-env }}
def --env gpenv-unload [] {{ hide-env --ignore-errors ...(gpenv unload --shell nu | from json) }}
gpenv-load test/entry
for n in [{names}] {{ print --no-newline $"($n)(char us)($env | get $n)(char rs)" }}
print --no-newline $"CHILD_SEES(char us)(sh -c 'printf %s "$GH_TOKEN"')(char rs)"
gpenv-unload
if ('PLAIN' in $env) or ('GPENV_LOADED' in $env) {{ error make {{msg: "unload left variables"}} }}
"#,
        names = expected()
            .iter()
            .map(|(n, _)| format!("'{n}'"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let out = run("nu", &["--no-config-file", "-c"], &script, &fx);
    check_values("nu", &out, &fx);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("CHILD_SEES\x1fshared-value"),
        "nu: not exported"
    );
}

#[test]
fn failure_prints_nothing_and_loads_nothing() {
    if !have("bash") {
        return;
    }
    let fx = fixture("fail");
    // Unknown entry: the stub fails, so gpenv must print nothing on stdout.
    let out = Command::new(bin())
        .args(["load", "test/missing"])
        .env("GPENV_GOPASS", fx.dir.0.join("gopass"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "stdout must be empty on failure");

    // Malformed entry: error names the line, never the value, and stdout is empty.
    std::fs::write(
        fx.dir.0.join("entry.txt"),
        "OK=1\nnot-an-assignment-supersecret\n",
    )
    .unwrap();
    let out = Command::new(bin())
        .args(["load", "test/entry"])
        .env("GPENV_GOPASS", fx.dir.0.join("gopass"))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(
        err.contains("line 2") && !err.contains("supersecret"),
        "{err}"
    );
}

#[test]
fn names_never_prints_values() {
    let fx = fixture("names");
    let out = Command::new(bin())
        .args(["names", "test/entry"])
        .env("GPENV_GOPASS", fx.dir.0.join("gopass"))
        .output()
        .unwrap();
    let s = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        s.lines().collect::<Vec<_>>(),
        expected().iter().map(|(n, _)| *n).collect::<Vec<_>>()
    );
    assert!(!s.contains("shared-value") && !s.contains("hello"));
}
