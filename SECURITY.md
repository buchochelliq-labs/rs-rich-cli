# Security policy

## Reporting a vulnerability

Please report a vulnerability **privately**, not in a public issue, pull
request or discussion.

Use GitHub's private vulnerability reporting: open the repository's
[Security tab](https://github.com/buchochelliq-labs/rs-rich-cli/security)
and choose **Report a vulnerability**, or go straight to
[a new advisory](https://github.com/buchochelliq-labs/rs-rich-cli/security/advisories/new).
Only the maintainers can see the report and the conversation that follows.

A useful report says:

- which crate, command or package, and which version (`rich --version`);
- what an attacker controls (a plugin, a marketplace listing, a web page,
  a program's output, a network position, another local account) and what
  they gain;
- how to reproduce it, ideally with a small input or a test;
- whether you plan to disclose it, and when.

## Supported versions

rs-rich is young and every crate is at `0.0.x`. Only **the latest
release** gets security fixes: the newest cohort listed in
[`RELEASES.toml`](RELEASES.toml) and on crates.io, the `rich` binary built
from it, and the newest `rs-rich` package on PyPI. A fix ships as a new
release of the affected crates; older releases are not patched.

| Version | Supported |
|---|---|
| The latest 0.0.x release | Yes |
| Any earlier release | No |

## Scope

In scope: the code in this repository, in particular the parts that run
other code or touch the network, as described in the
[threat model](docs/design/threat-model.md):

- plugin loading, native and WASM, and the WASM sandbox;
- the marketplace client (`rs-rich-marketplace`, `rich plugin`), its
  signature, key-chain, lockfile and install checks, and the marketplace's
  signing and listing workflows kept in
  [`docs/marketplace-repo/`](docs/marketplace-repo/);
- `rich serve` and `rs-rich-web`;
- Noctiluca and rs-rich-embed's Chrome engine, web view and PTY host;
- TLS, certificate and redirect handling;
- terminal-control injection: untrusted input that reaches the user's
  terminal as escape sequences where the docs say it is sanitized;
- the Python bindings (`rs-rich` on PyPI).

Out of scope:

- behaviour the docs describe as trust-only: a native plugin runs as the
  user, and a program run with `rich serve` can do what that program does
  for anyone holding the token;
- a server exposed beyond the machine without the reverse proxy, TLS and
  authentication the docs require;
- upstream Python `rich` and `rich-cli` (report those
  [upstream](https://github.com/Textualize/rich/security)), and modes that
  reproduce upstream's output unchanged, unless the port makes it worse;
- bugs in Chrome, a terminal emulator, the operating system or a
  dependency, unless rs-rich makes them reachable where they otherwise
  would not be (please report those upstream as well);
- the code of third-party plugins themselves, except where it escapes the
  sandbox or the host's checks.

When unsure, report it anyway.

## What to expect

rs-rich is maintained by a small team, without a bug bounty. We aim to:

- acknowledge a report within a week;
- agree with you on whether it is a vulnerability, and on its severity;
- fix it in a release, and publish a GitHub security advisory with the
  fix, crediting you unless you prefer otherwise.

We ask that you give us a reasonable time to ship the fix before disclosing
it publicly, and we will keep you informed while we work on it.
