# gpenv: why it exists, and its security model

Background for anyone evaluating gpenv. The [README](../README.md) has the short version.

- [Why this exists](#why-this-exists)
- [Security model](#security-model)

## Why this exists

### The starting point: secrets in shell files

Most people's work environment starts out as a shell function in a dotfiles
repo that exports the tokens and passwords a day's work needs:

```sh
load-work-keys() {
  export GITHUB_TOKEN="ghp_…"          # literal secret in a file
  export JIRA_API_TOKEN="ATATT…"
  export DATADOG_APIKEY="9c41…"
  …
}
```

It's convenient, and then the dotfiles repo gets pushed. Every secret is now
in git history on a remote, and rotating them all is the only real fix.

### The usual fix: one gopass lookup per secret

The standard remedy is to keep secrets in [gopass](https://www.gopass.pw/) and
look them up at load time:

```sh
load-work-keys() {
  export GITHUB_TOKEN=$(gopass show -o work/github-token)
  export JIRA_API_TOKEN=$(gopass show -o work/jira-token)
  export DATADOG_APIKEY=$(gopass show -o work/datadog-api-key)
  … # 20 more
}
```

That fixes the leak, but creates three new problems.

**1. It's slow.** Each `gopass show` is a separate GPG decryption. Measured on
an Apple Silicon Mac with gpg-agent already unlocked:

| | Time |
|---|---|
| one `gopass show -o` | ~180 ms |
| the same file with plain `gpg --decrypt` | ~176 ms |
| gopass's own startup (`gopass --version`) | ~66 ms |
| **21 lookups in a row** | **~3.5 s** |

So almost all the time is GPG doing a public-key decryption per entry. That's
why "rewrite the loader in a faster language" doesn't help: a Rust program
making 21 decrypts is just as slow. The only real fix is **fewer decrypts**.

**2. It fails silently.** If one lookup fails (wrong path, locked agent,
cancelled pinentry), `$(…)` returns an empty string and the variable is set
to `""`, or keeps a stale value from earlier. The function still prints "loaded".

**3. It only works in POSIX shells.** The same function can't be sourced in
nushell, and keeping a second copy per shell means they drift.

### What gpenv does instead

gpenv keeps **all the variables in one gopass entry**:

```text
GITHUB_TOKEN,GH_TOKEN=ghp_…
JIRA_API_TOKEN=ATATT…
DATADOG_APIKEY,TF_VAR_DATADOG_APIKEY=9c41…
JIRA_PROJECT_KEY=OPS
```

and turns it into your shell's syntax:

- **One decrypt.** `gpenv load` runs `gopass show` once, so loading takes
  about 0.2–0.3 s however many variables there are. In real use, 41
  variables went from ~3.5 s to ~0.3 s.
- **All or nothing.** If gopass fails or any line is malformed, gpenv prints
  nothing and exits 1, so the wrapper changes nothing. No half-loaded shells,
  no empty secrets that look loaded.
- **One file, every shell.** The same entry loads into bash and zsh (`eval`
  of single-quoted exports) and nushell (`from json | load-env`), on macOS
  and Linux, from one small binary.
- **Safe to `eval`.** Values are literal: no `$VAR` expansion and no
  command substitution. Every value is single-quoted with `'` escaped as `'\''`,
  so a value like `$(rm -rf ~)` stays text. Tests run hostile values
  through real bash, zsh and nushell and check nothing executes.
- **No secrets in output you didn't ask for.** Errors name the line and
  variable, never the value. `gpenv names` lists names only. `Debug` output
  hides values.
- **Unloadable.** gpenv records which names it set (in `GPENV_LOADED`), so
  `gpenv-unload` removes exactly those, and the secrets don't linger in a long-lived shell.
- **One place to rotate.** A secret lives on exactly one line, so rotating
  it is one `gopass edit`. Aliases (`GITHUB_TOKEN,GH_TOKEN=…`) mean no
  duplicate copies to forget.

Nothing is written to disk. Secrets go from the encrypted store straight
into your shell's environment.

### What it doesn't do

- It doesn't replace gopass or GPG. It's a thin layer that calls `gopass show` once.
- It can't protect secrets once they're in your shell: every program you
  start inherits them. Use `gpenv-unload`, or load in a subshell for one command.
- It doesn't sync anything. gopass's own git sync already does that.

## Security model

### What gpenv protects against

| Risk | How gpenv handles it |
|---|---|
| Secrets committed to a dotfiles repo, or left in plaintext files | Secrets live only in the encrypted gopass store and go straight into the shell's environment. Nothing is written to disk |
| Stale or duplicated copies after a rotation | Each secret is on exactly one line; `A,B=value` gives one value several names, so rotation is a single edit |
| A failed load that looks successful (empty or stale variables) | **Fails closed:** if gopass fails or any line is malformed, gpenv prints nothing and exits 1, so the shell is unchanged |
| A value being executed when the output is `eval`-ed | Values are always literal and single-quoted (`'` becomes `'\''`). Tests run `$(…)`, backticks, `;`, quotes and newlines through real bash, zsh and nushell and check nothing runs |
| Secrets shown in errors, listings or logs | Errors name the line and variable, never the value. `gpenv names` prints names only. `Debug` output hides values |
| Secrets lingering in a long-lived shell | `gpenv-unload` removes exactly the names gpenv set (tracked in `GPENV_LOADED`) |
| Supply-chain risk in the tool itself | No dependencies, `#![forbid(unsafe_code)]`, under 500 lines of Rust including its unit tests |

### What gpenv does NOT protect against

- **Anything after loading.** Once exported, secrets are in your shell's
  environment and **every program you start inherits them**. Any process
  running as you can read another process's environment on most systems.
  Load in a subshell for one command when you can:
  `( gpenv-load work/env && terraform plan )`.
- **A compromised account or machine.** Anyone who can run commands as you
  can run `gopass show` too.
- **Weaknesses in gopass or GPG.** gpenv relies entirely on them for
  encryption, key management and access to the store.
- **Copies outside gpenv's control.** gpenv overwrites its own copy of the
  decrypted text before exiting, as a best effort. Copies in gopass,
  gpg-agent and your shell are out of its reach.
- **Organisational controls.** There's no access control, audit log,
  expiry or rotation enforcement (see below).

### Compliance

gpenv is **not a compliance solution** and doesn't claim to meet any standard.

It can help with good practice, such as keeping credentials out of source
code and plaintext files. That's the kind of control secret-handling
requirements in frameworks like SOC 2, ISO/IEC 27001 and PCI DSS ask for. But
those frameworks certify an organisation's processes, and they typically
require things gpenv deliberately doesn't provide:

- central, per-person access control and revocation;
- an **audit log** of who accessed which secret, and when;
- enforced rotation and expiry;
- documented key management.

If you have compliance obligations, use a managed secrets manager (for
example HashiCorp Vault, AWS Secrets Manager or 1Password) under your
organisation's policies, and treat gpenv as a convenience for personal
workstations at most.

### Reporting a vulnerability

See [SECURITY.md](../SECURITY.md).
