# Security policy

## Reporting a vulnerability

Please report security issues **privately**, not in a public issue.

Use GitHub's private vulnerability reporting: go to the repository's
**Security** tab → **Report a vulnerability**. Only the maintainer can see the report.

Include, as far as you can:
- what the issue is and its impact;
- steps or input to reproduce it (please use **fake** secrets);
- the gpenv version (`gpenv --version`), OS and shell.

You can expect an acknowledgement within a week. gpenv is maintained by one
person in their spare time, so there's no formal SLA, but security reports
come first.

## Scope

In scope, for example:
- gpenv output that executes something when `eval`-ed by bash or zsh, or when
  loaded by nushell (a value escaping its quoting);
- a secret value appearing in gpenv's stdout when it shouldn't, in error
  messages, or in `gpenv names` / `Debug` output;
- gpenv exporting a partial or wrong set of variables instead of failing;
- an entry name being interpreted as a gopass option.

Out of scope (see [security model](docs/why-and-security.md#security-model)):
- secrets being visible to other processes once they're in your shell's
  environment;
- issues in gopass, GPG or your shell itself; please report those upstream;
- attacks that already require running commands as your user.

## Supported versions

Only the latest release on the `main` branch gets security fixes.
