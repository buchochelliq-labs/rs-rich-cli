<!--
  The four ## sections below are MANDATORY and are checked by the
  "pr hygiene" CI job. Replace every <!-- ... --> prompt with real content —
  leaving a prompt in place fails the check, deliberately: a template filled with
  its own placeholder text is worse than no template.

  "N/A — <reason>" is always an acceptable answer. Saying why something does not
  apply is information; silence is not.
-->

## What & why

<!-- What changes, and what problem it solves. Link the issue: "Closes #123", or
     say "no issue" and why. If a reviewer would ask "why is this needed?", the
     answer belongs here. -->

## How this was verified

<!-- What you actually ran, and what it showed. Not "tests pass" — which tests,
     and what they would have caught. If you verified behaviour against upstream
     Python rich, give the command and the bytes.

     If this cannot be verified automatically, say so and explain what you did
     instead. -->

## Parity impact

<!-- Does this change rendered output relative to upstream rich 15.0.0?

     - "None — internal only."
     - "Fixes a divergence: <before> -> <after>, verified against real rich."
     - "Introduces a deliberate divergence" — then docs/DIVERGENCES.md MUST be
       updated in this PR, and you should say why it is unavoidable.

     Golden fixtures moving is a red flag unless this is a parity FIX. If any
     fixture changed, explain which and why. -->

## Risk

<!-- What could this break, and what would tell us? Public API changes, changed
     defaults, new dependencies, anything irreversible once published.
     "Low — additive, no public API change" is a fine answer when true. -->

---

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all` all pass
- [ ] Golden fixtures regenerate byte-identically (`python scripts/capture_golden.py` leaves `git diff` clean), or the change to them is explained above
- [ ] Docs updated where the change makes them wrong (README / AGENTS.md / docs/*.md / CHANGELOG.md)
- [ ] Public API changes are intentional and noted above
- [ ] **This PR targets `main`** — or, if it is stacked on another PR's branch, that is stated above *and* the base will be retargeted to `main` before merge

<!--
  That last box is not bureaucracy. This repo has lost work twice to stacked
  branches: a PR merged into a base branch whose own PR had already
  squash-merged, so it reported "merged" while its commits never reached main.
  See docs/BRANCHING.md.
-->
