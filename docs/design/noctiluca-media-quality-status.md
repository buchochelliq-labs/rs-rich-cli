# Noctiluca media quality: handoff

Updated: 2026-11-08.

## Working state

- Branch: `codex/noctiluca-media-quality`; design baseline `7fcdbf7263f81d89c2975a47ee4761b4a0e1c7cc`.
- Tasks 1–5 are implemented and reviewed. Task 6 source, terminal feature policy and fitted-picture resource work is in progress; its WIP commits are not final acceptance.
- The shared image presentation path opts into distinct fitted/versioned Kitty resources, capability-aware animation and supported fallback formats. Legacy low-level picture IDs remain available. Manual Driver loops must acknowledge successfully written output as documented in the image guide.
- Native Kitty, WezTerm, tmux and Xterm captures exist for earlier source revisions; final-source captures and the full terminal/transport matrix remain open. Ghostty, macOS iTerm2 and SSH are unverified on this host.
- Final release-profile paired timings, full workspace checks, and Task 6 and whole-branch reviews remain open. Earlier workspace clippy failed on existing faithful-rich warnings and workspace test linking exhausted disk; these are not passing final checks.

## Next

Finish focused lifecycle and public-consumer checks, freeze and preserve a final executable, then capture native evidence and run the matched isolated release comparisons. Complete the required workspace checks and independent reviews; record unavailable combinations and any failures without inferring native behavior from escape bytes.
