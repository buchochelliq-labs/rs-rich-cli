# Configurable Action Labels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make full action-label wrapping configurable through shared intuiTUIve controls and Noctiluca adaptations.

**Architecture:** Add one bool builder to existing stock buttons, then adopt those measured controls in Noctiluca semantic popup items. Keep compact disclosure and scrolling composed from existing public primitives; browser identities remain in the leaf adapter.

**Tech Stack:** Rust, intuiTUIve controls/layout/scroll/layers, Noctiluca adapt/config/keymap and guarded source actions.

**Spec:** `docs/superpowers/specs/2026-11-08-configurable-action-labels-design.md`.

## Global Constraints

- User requires configuration: both wrapped and compact modes must work. Shared defaults remain compact; Noct adaptive default is wrapped and faithful mode disables it.
- No new required public fields or browser types in shared core; no faithful rich edits, site rules or duplicate private button implementation.
- Preserve full source names, original node/revision/hints/action routes, disabled guards, password exclusion, single focus target and source-owned Escape.
- Wrapped source popups must actually use shared stock buttons and measured semantic items, not taller text across duplicate per-row targets.
- Preserve source-fitted clipping, ordinary underlay alignment, semantic item anchors, independent scrolling and reachable large content.
- One source/Cargo implementer; no worker helpers/reviewers, external activation, push/merge/publish. Root coordinates native/quiet leases.

## Review Focus

- A label growing after initial measurement must enlarge the control and scroll reveal its whole target (Task1 and Task2).
- Literal brackets, combining marks, CJK and long words must wrap in terminal cells without markup injection or broken wide halves (Task1).
- Multiple visual rows must remain one guarded source action and one Tab stop (Task2).
- Switching modes/resize must preserve semantic focus/anchor and remove the old target (Task2).
- Disabled names, large dialogs and local details dismissal must remain readable/reachable without accidental source action (Task2).

---

### Task 1: Configurable shared button-label wrapping

**Files:** Modify/test `crates/rich-intuituive/src/controls.rs` and the demonstrated focused-geometry reveal fix in `builtin.rs`; extend existing control interaction tests and framework-only external consumer; document `docs/guide/intuituive/controls.md`.

**Interfaces:** Add public `Node::wrap_label(self, enabled: bool) -> Node`; private ButtonWidget state defaults false. Constructors/signatures/public literals unchanged. Other node kinds are unaffected.

- [x] Add intended RED for a narrow opted-in button: complete long suffix across measured rows, larger vertical measure and clickable lower rows. Keep a default compact control as parity reference. Cover literal `[draft]`, CJK, combining text, long word, tiny widths, live label/resize, Enter/Space, mouse release inside/outside and disabled guard.
- [x] Implement the bool builder and one shared wrapped measurement/draw layout using existing text/Unicode machinery. Preserve legacy compact path and decoration/state/theme semantics. No new rendering engine or duplicate layout in measure/draw.
- [x] Verify focused control tests, modified library suite, fmt/scoped strict lint and an isolated default-features-disabled public consumer using both bool values. Record intended RED/GREEN and any proven baseline failures. Document the API and compact disclosure composition.
- [x] Commit/self-review/report exact BASE/HEAD and evidence. Root independent task review before Noct adoption.

### Task 2: Noct configuration and true popup adoption

**Files:** Modify `crates/noctiluca/src/{adapt,layout,widgets,app,actions}.rs`; add focused private `app/labels.rs` if needed to keep new composition readable; config/CLI/keys/popup tests and existing Noct guide sections.

**Interfaces:** Append `Rule::WrapActionLabels`, register name `wrap-action-labels`, default true, existing Config.adapt and CLI +/-; add action ID `page.labels`, default `shift+f1` both presets via existing configurable keymap. Consume Task1 `wrap_label(bool)`. No public model/Config field additions.

- [ ] Add config/CLI/default/faithful tests and neutral40-column source-dialog RED with long buttons/hints/disabled action. Assert real stock-button adoption, one Tab stop per enabled source ID, full suffix reachability, exact guarded actions and no duplicate underlying row-zone target. Include live labels, mode toggle/resize, focus and scroll anchor.
- [ ] Implement full-name projection policy and stock adapter adoption. In source popups replace action faces with measured semantic item/action groups containing shared wrapped buttons; retain other adapters/paint sampling and source-fitted viewport. Use bounded measured scroll or existing variable-height virtual scroll without row-cap loss. Do not fake adoption by only changing widgets.rs.
- [ ] Add compact tooltips and `page.labels` read-only scrollable details with disabled names, configurable binding, F1 unchanged, no password values and no source action/Escape on local dismissal. Preserve source owner dismissal and navigation cleanup.
- [ ] Verify ordinary and beyond4000-row content navigation, hints, disabled/readonly/source revision guards, owner dismissal and existing product/video/popups. Run Noct suite/fmt/scoped lint; explicitly document public Rule-variant extension and adaptive default/config behavior.
- [ ] Coordinate native capture: both modes on neutral fixture at40columns and a reachable ordinary popup; no external consent/action activation. Preserve source/config/hash/screenshots, self-review and commit/report. Root task review and later broad continuation review/application gates remain required.


Shared Task1 independently reviewed PASS atbc0b261:1474unit/integration,408doctests and2isolated consumer tests passed. The shared ordinary-scroll fix re-reveals a focused target when its content geometry/viewport changes; unchanged-geometry manual scrolling stays put. Noct adoption remains Task2 and is not implemented by the core result.
