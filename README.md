# gpenv

> Load env vars from a single gopass entry into bash, zsh or nushell, with
> one decrypt instead of one per secret. A tiny Rust CLI with no
> dependencies, for macOS and Linux.

```sh
gpenv-load work/env      # ~0.3 s: every variable exported into this shell
gpenv-unload             # gone again
```

## Why

Keeping secrets in gopass and loading them with one `gopass show` per
variable works, but each lookup is a separate GPG decryption (about 180 ms).
21 lookups took **about 3.5 s** every time. gpenv keeps all the variables in
**one** entry, so loading is one decrypt: **about 0.3 s** for 41 variables. The
same entry works in bash, zsh and nushell, on macOS and Linux.

Details, measurements and the full security model:
[docs/why-and-security.md](docs/why-and-security.md).

## How is this different from `gopass env`?

gopass has a built-in `gopass env`, and it's easy to think gpenv duplicates
it. They solve different problems:

| | `gopass env` (built in) | gpenv |
|---|---|---|
| Layout | One entry per variable: `work/env/github_token` → `GITHUB_TOKEN` | Many `NAME=value` lines in one entry |
| Decrypts | One per variable | One for the whole set |
| Result | Runs **one command** with the variables set | Loads them into **your current shell**, and unloads them again |
| Names | Entry name, uppercased | Exactly as written; `A,B=value` gives one value several names |
| Shells | Any (it starts the command itself) | bash, zsh, nushell |
| Extras | `--file` (secret in a temp file), `--stdin` | Values never shown in errors; any failure loads nothing |

`gopass env` reads only the **first line** of each entry, so it can't use
gpenv's one-entry format, and gpenv can't read a `gopass env` folder.

Use `gopass env work/env -- terraform plan` for a single command with a few
secrets: nothing stays in your shell afterwards. Use gpenv when you want many
variables in an interactive shell, quickly, and in nushell too.

## Entry format

Store the variables in a single gopass entry, e.g. `work/env`:

```text
# Work environment: loaded with  gpenv-load work/env
JIRA_PROJECT_KEY=OPS
JIRA_API_TOKEN=ATATT...
GITHUB_TOKEN,GH_TOKEN=ghp_...            # one value, several names
TF_HTTP_PASSWORD,GITLAB_ACCESS_TOKEN=glpat-...
JIRA_TRANSITION_RESOLVE=In Progress,Review,Ready,Done
export EDITOR=vim                        # "export " is optional
```

Rules:
- One `NAME=value` per line. Blank lines and `#` comments are ignored.
- Names are `[A-Za-z_][A-Za-z0-9_]*`. Use `A,B=value` to set several names to one value.
- **Values are literal.** There's no `$VAR` expansion, no `$(…)`, and no
  escapes; everything after the first `=` is the value. One pair of matching
  surrounding quotes is removed, so `X="a b"` and `X=a b` are the same.
- Each value is on one line.
- `GPENV_LOADED` is reserved. gpenv uses it to remember which names it set.

Edit it with `gopass edit work/env`. List the names without values with
`gpenv names work/env`.

## Shell setup

**bash / zsh**: in `~/.bashrc` or `~/.zshrc`:

```sh
gpenv-load()   { local out; out=$(command gpenv load "$1" --shell sh) && eval "$out"; }
gpenv-unload() { local out; out=$(command gpenv unload --shell sh) && eval "$out"; }
```

**nushell**: in `config.nu`:

```nu
def --env gpenv-load [entry: string] { gpenv load $entry --shell nu | from json | load-env }
def --env gpenv-unload [] { hide-env --ignore-errors ...(gpenv unload --shell nu | from json) }
```

Then, in any of the three:

```sh
gpenv-load work/env          # export everything
gpenv-unload                 # remove it again
```

A program can't change its parent shell's environment, so gpenv prints
commands (sh) or JSON (nu), and the wrapper applies them.

## Commands

| Command | Output |
|---|---|
| `gpenv load ENTRY --shell sh` | `export NAME='value'` lines, plus `GPENV_LOADED` |
| `gpenv load ENTRY --shell nu` | a JSON record for `from json \| load-env` |
| `gpenv unload --shell sh\|nu` | `unset …` / a JSON list for `hide-env` |
| `gpenv names ENTRY` | variable names only, one per line |

On any error, gpenv writes **nothing** to stdout and exits 1. Errors name
the line and variable, never the value.

`GPENV_GOPASS=/path/to/gopass` overrides which gopass binary is used.

## Install

Needs a Rust toolchain (≥ 1.85) and `gopass` on `PATH`. There are no other dependencies.

```sh
cargo install --locked --path .          # from a checkout
```

The binary is self-contained. Build it once per OS/architecture (macOS
arm64, Linux x86_64/arm64) and copy it to `~/.local/bin` or `/usr/local/bin`.

> **Formerly `genv`, then briefly `gopass-env`.** `genv` clashed with GNU
> `env`, which Homebrew's `coreutils` installs as `genv`; `gopass-env` was too
> easy to mistake for gopass's built-in `gopass env`. If you used an old name,
> delete `~/.cargo/bin/genv` / `~/.cargo/bin/gopass-env`, rename the wrappers
> to `gpenv-load` / `gpenv-unload`, and open a fresh shell (the old
> `GENV_LOADED` / `GOPASS_ENV_LOADED` lists aren't read any more).

## Security

- **Nothing on disk:** secrets go from the encrypted gopass store straight into your shell.
- **Fails closed:** on any error gpenv prints nothing, so the shell is unchanged.
- **Safe to `eval`:** values are literal and single-quoted; nothing inside a value runs.
- **Values are never printed** in errors, `gpenv names` or debug output.
- **Not protected after loading:** every program you start inherits the
  variables. Use `gpenv-unload`, or a subshell for one command.
- **Not a compliance solution:** there's no access control, audit log or rotation enforcement.

More: [security model](docs/why-and-security.md#security-model).
Report vulnerabilities privately: [SECURITY.md](SECURITY.md).

## Development

```sh
cargo test        # unit tests + end-to-end tests in real bash, zsh and nu (skipped if not installed)
cargo clippy --all-targets -- -D warnings
```

## Acknowledgements

gpenv is a thin layer on top of other people's work:

- **[gopass](https://github.com/gopasspw/gopass)**, the team password manager
  that stores and syncs the secrets. gpenv only calls `gopass show`; all the
  encryption, storage, git sync and key management is gopass.
- **[pass](https://www.passwordstore.org/)** by Jason A. Donenfeld, the
  original "standard unix password manager" that gopass builds on.
- **[GnuPG](https://gnupg.org/)**, which does the actual decryption.

gpenv is an independent project and isn't affiliated with or endorsed by the
gopass project.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
