//! Parse a gopass entry of `NAME=value` lines and render it for a shell.
//!
//! Entry format, one assignment per line:
//!
//! ```text
//! # comments and blank lines are ignored
//! JIRA_PROJECT_KEY=OPS
//! export EDITOR=vim                     # "export " prefix is optional
//! GITHUB_TOKEN,GH_TOKEN=ghp_xxx         # one value, several names
//! GREETING="hello world"                # one pair of surrounding quotes is removed
//! ```
//!
//! Values are **literal**: no `$VAR` expansion, no escapes, no command
//! substitution. Anything after the first `=` (minus one pair of matching
//! surrounding quotes) is the value. That's what makes the output safe to
//! `eval`: the renderer quotes it, and nothing in a value is ever run.
//!
//! Error messages name the line number and variable, never the value.

#![forbid(unsafe_code)]

use std::fmt;

/// Variable that records which names gpenv exported, so `gpenv unload` can
/// remove them again. It holds names only, never values.
pub const LOADED_VAR: &str = "GPENV_LOADED";

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub reason: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

impl std::error::Error for ParseError {}

/// One variable to export.
#[derive(Clone, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    pub value: String,
}

/// Values are secrets: Debug shows the name only.
impl fmt::Debug for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Var({} = <{} bytes>)", self.name, self.value.len())
    }
}

/// Overwrites the value's bytes before freeing, as a best effort.
impl Drop for Var {
    fn drop(&mut self) {
        // Take the String, zero its bytes, then let it drop.
        let mut bytes = std::mem::take(&mut self.value).into_bytes();
        bytes.iter_mut().for_each(|b| *b = 0);
        std::hint::black_box(&bytes);
    }
}

/// `[A-Za-z_][A-Za-z0-9_]*`: a name every shell (including nushell) accepts.
pub fn valid_name(n: &str) -> bool {
    let mut b = n.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

pub fn parse(text: &str) -> Result<Vec<Var>, ParseError> {
    let mut out: Vec<Var> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let err = |reason: String| ParseError {
            line: line_no,
            reason,
        };
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim_start();
        if trimmed.trim().is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let body = trimmed
            .strip_prefix("export ")
            .map_or(trimmed, str::trim_start);
        let (names, value) = body
            .split_once('=')
            .ok_or_else(|| err("expected NAME=value".into()))?;
        let value = unquote(value);
        if value.contains('\0') {
            return Err(err("value contains a NUL byte".into()));
        }
        for name in names.split(',').map(str::trim) {
            if !valid_name(name) {
                return Err(err(format!(
                    "{name:?} is not a valid variable name ([A-Za-z_][A-Za-z0-9_]*)"
                )));
            }
            if name == LOADED_VAR {
                return Err(err(format!("{LOADED_VAR} is reserved by gpenv")));
            }
            if out.iter().any(|v| v.name == name) {
                return Err(err(format!("{name} is set twice")));
            }
            out.push(Var {
                name: name.to_owned(),
                value: value.to_owned(),
            });
        }
    }
    Ok(out)
}

/// Removes one pair of matching surrounding quotes, without processing escapes.
fn unquote(v: &str) -> &str {
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

/// Names from `GPENV_LOADED` plus `new`, deduplicated, in first-seen order.
pub fn merge_loaded(existing: Option<&str>, new: &[Var]) -> Vec<String> {
    let mut names: Vec<String> = existing
        .unwrap_or("")
        .split_whitespace()
        .filter(|n| valid_name(n))
        .map(str::to_owned)
        .collect();
    for v in new {
        if !names.contains(&v.name) {
            names.push(v.name.clone());
        }
    }
    names
}

/// Single-quotes a string for POSIX shells: `'` becomes `'\''`. Inside single
/// quotes, sh/bash/zsh interpret nothing, so this is safe to `eval`.
pub fn sh_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// A JSON string literal (RFC 8259), for nushell's `from json`.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// bash, zsh, dash, ksh: `export NAME='value'` lines for `eval`.
    Sh,
    /// nushell: a JSON record for `from json | load-env`.
    Nu,
}

impl std::str::FromStr for Shell {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "sh" | "bash" | "zsh" | "posix" => Ok(Self::Sh),
            "nu" | "nushell" | "json" => Ok(Self::Nu),
            other => Err(format!("unknown shell {other:?}: use sh, bash, zsh or nu")),
        }
    }
}

/// Output for `gpenv load`.
pub fn render_load(vars: &[Var], loaded: &[String], shell: Shell) -> String {
    let loaded = loaded.join(" ");
    match shell {
        Shell::Sh => {
            let mut out = String::new();
            for v in vars {
                out.push_str(&format!("export {}={}\n", v.name, sh_quote(&v.value)));
            }
            out.push_str(&format!("export {LOADED_VAR}={}\n", sh_quote(&loaded)));
            out
        }
        Shell::Nu => {
            let mut fields: Vec<String> = vars
                .iter()
                .map(|v| format!("{}:{}", json_string(&v.name), json_string(&v.value)))
                .collect();
            fields.push(format!(
                "{}:{}",
                json_string(LOADED_VAR),
                json_string(&loaded)
            ));
            format!("{{{}}}\n", fields.join(","))
        }
    }
}

/// Output for `gpenv unload`: removes every name in `GPENV_LOADED`, then itself.
pub fn render_unload(loaded: Option<&str>, shell: Shell) -> String {
    let mut names: Vec<&str> = loaded
        .unwrap_or("")
        .split_whitespace()
        .filter(|n| valid_name(n))
        .collect();
    names.push(LOADED_VAR);
    match shell {
        Shell::Sh => format!("unset {}\n", names.join(" ")),
        Shell::Nu => format!(
            "[{}]\n",
            names
                .iter()
                .map(|n| json_string(n))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[Var]) -> Vec<&str> {
        v.iter().map(|v| v.name.as_str()).collect()
    }

    #[test]
    fn parses_all_forms() {
        let v = parse(
            "# Work\n\nA=1\nexport B=two words\nGITHUB_TOKEN,GH_TOKEN=ghp_x\nQ=\"quoted\"\nS='single'\nE=\nU=a=b=c\r\n  # indented comment\n",
        )
        .unwrap();
        assert_eq!(
            names(&v),
            ["A", "B", "GITHUB_TOKEN", "GH_TOKEN", "Q", "S", "E", "U"]
        );
        let val = |n: &str| v.iter().find(|x| x.name == n).unwrap().value.clone();
        assert_eq!(val("B"), "two words");
        assert_eq!(val("GH_TOKEN"), "ghp_x");
        assert_eq!(val("Q"), "quoted");
        assert_eq!(val("S"), "single");
        assert_eq!(val("E"), "");
        assert_eq!(val("U"), "a=b=c", "split on the first = only; CR stripped");
    }

    #[test]
    fn values_are_literal() {
        let v = parse("X=$(rm -rf ~) `id` $HOME \\n 'a'\"b\"").unwrap();
        assert_eq!(v[0].value, "$(rm -rf ~) `id` $HOME \\n 'a'\"b\"");
        // Mismatched quotes are kept as-is.
        assert_eq!(parse("Y=\"abc'").unwrap()[0].value, "\"abc'");
    }

    #[test]
    fn errors_name_the_line_never_the_value() {
        for (input, want) in [
            (
                "A=1\nsupersecret-without-equals",
                "line 2: expected NAME=value",
            ),
            ("1BAD=supersecret", "line 1: \"1BAD\" is not a valid"),
            ("A-B=supersecret", "line 1: \"A-B\""),
            ("A=1\nA=supersecret", "line 2: A is set twice"),
            ("A,A=supersecret", "line 1: A is set twice"),
            ("GPENV_LOADED=supersecret", "reserved"),
            ("A=super\0secret", "NUL"),
        ] {
            let e = parse(input).unwrap_err().to_string();
            assert!(e.contains(want), "{input:?} -> {e}");
            assert!(
                !e.contains("supersecret") && !e.contains("super\0"),
                "value leaked: {e}"
            );
        }
    }

    #[test]
    fn debug_hides_values() {
        let v = parse("TOKEN=hunter2").unwrap();
        assert!(!format!("{v:?}").contains("hunter2"));
    }

    #[test]
    fn sh_quoting() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn json_escaping() {
        assert_eq!(
            json_string("a\"b\\c\nd\te\u{1}"),
            "\"a\\\"b\\\\c\\nd\\te\\u0001\""
        );
        assert_eq!(json_string("ünï ✓"), "\"ünï ✓\"");
    }

    #[test]
    fn loaded_list_merges_and_dedupes() {
        let v = parse("B=1\nC=2").unwrap();
        assert_eq!(merge_loaded(Some("A B bad-name"), &v), ["A", "B", "C"]);
        assert_eq!(merge_loaded(None, &v), ["B", "C"]);
    }

    #[test]
    fn render_shapes() {
        let v = parse("A=x\nB=it's").unwrap();
        let loaded = merge_loaded(None, &v);
        assert_eq!(
            render_load(&v, &loaded, Shell::Sh),
            "export A='x'\nexport B='it'\\''s'\nexport GPENV_LOADED='A B'\n"
        );
        assert_eq!(
            render_load(&v, &loaded, Shell::Nu),
            "{\"A\":\"x\",\"B\":\"it's\",\"GPENV_LOADED\":\"A B\"}\n"
        );
        assert_eq!(
            render_unload(Some("A B"), Shell::Sh),
            "unset A B GPENV_LOADED\n"
        );
        assert_eq!(render_unload(None, Shell::Nu), "[\"GPENV_LOADED\"]\n");
        assert_eq!(
            render_unload(Some("A $(id)"), Shell::Sh),
            "unset A GPENV_LOADED\n",
            "invalid names dropped"
        );
    }

    #[test]
    fn shell_names() {
        for s in ["sh", "bash", "zsh", "posix"] {
            assert_eq!(s.parse::<Shell>(), Ok(Shell::Sh));
        }
        assert_eq!("nu".parse::<Shell>(), Ok(Shell::Nu));
        assert!("fish".parse::<Shell>().is_err());
    }
}
