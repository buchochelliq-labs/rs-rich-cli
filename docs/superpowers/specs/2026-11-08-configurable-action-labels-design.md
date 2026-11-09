# Configurable action labels

User decision: **“Make it configurable.”** This extends the [approved media-quality design](2026-11-08-noctiluca-media-quality-design.md). Long source-popup choices must have a readable wrapped presentation and a compact presentation, using public intuiTUIve controls. No site-specific layout or parallel private button implementation is acceptable.

## Shared contract

Add `Node::wrap_label(self, enabled: bool) -> Node` for stock buttons, ignored on other node types. Existing constructors/defaults remain compact. The private button implementation uses one width-aware text layout for wrapped measurement and drawing, with the existing markup/Unicode text machinery, long-word folding, and safe tiny-width behavior. Complete accessible names, variants/themes, disabled guards, focus, keyboard activation and pointer capture/release remain intact. Wrapped content takes precedence over decorative edge rows. Live labels and width changes must invalidate measured height.

A demonstrated shared scroll gap is part of this contract: a focused control that grows or is clipped by a shrinking viewport must be revealed again. Track content geometry and viewport changes, while preserving deliberate wheel, page-key and programmatic scrolling when that geometry stays unchanged. This belongs in the generic scroll component, without a browser focus workaround or new public scroll API.

The bool builder is the reusable programmatic configuration. Do not also add a duplicate no-argument builder, new required options field or global App preference. Compact full-name access uses existing tooltip/modal/scroll primitives; no new disclosure widget is missing.

## Noctiluca policy

Append `Rule::WrapActionLabels` without moving existing rule ordinals; register `wrap-action-labels` through existing `Config.adapt`, CLI adaptation +/- and About. The adaptive Noctiluca default is wrapped; `[adapt] wrap-action-labels = false` selects compact. Faithful mode/`--no-adapt` disables wrapping. Shared library button defaults stay unchanged. Document the new public Rule variant for exhaustive-match consumers; there is no new required Config/Page field.

Wrapped action labels use the complete source name, rather than an icon-only abbreviation, preserve original node/revision/hint/action identity, and use public stock `button_of(...).wrap_label(true)` in source popups and the existing widget adapter. Text fields, checks, media and arbitrary source content keep their appropriate adapters. Main projection uses existing shared text layout for full action faces rather than truncating them back to one line. Source ownership, background/foreground ordering and image containment remain authoritative.

A source popup must have **one focusable control per original enabled source action**, regardless of label rows. Replace the corresponding old face/hit target; do not layer a second active button over row-zone leaves. Compose semantic paint/content items and complete action groups using shared measured columns/rows/adaptive columns and existing scrolling. Retained paint rows may still use original row coordinates for source image sampling; their placement follows measured item positions. Keep the source-fitted outer viewport and independent scroll/owner-controlled modal behavior.

Use ordinary measured scroll for bounded content and existing variable-height virtual scrolling for large content; do not introduce a 4000-row truncation or claim unbuilt virtual nodes are already in the focus tree. Existing keyboard page navigation must reach all content. Prove normal-popup Tab order and focus reveal, and preserve the large-popup virtual path without duplicate targets. A genuinely missing reusable focus primitive requires a separate demonstrated shared fix, not a browser-only workaround.

Compact mode retains density and source faces, while focused/hovered popup targets expose their full names. Add a described, configurable `page.labels` action, default `shift+f1` in both presets, opening a read-only scrollable labels popup composed from existing shared primitives. F1 keeps Actions. Include disabled action names without enabling them; never display password values. Closing local details sends no source Escape and restores source scope/focus. Source popup Escape still belongs to its source owner.

Mode changes preserve the anchored semantic item and source identity rather than reinterpreting old paint-row offsets as measured-item offsets. Closing/navigating the source owner retires its local detail state safely. Source-positioned underlay remains spatially faithful while a source popup is open; label preference governs the popup's own content, not movement of the page behind it.

## Proof

A framework-only public consumer tests default versus wrapped buttons, full Unicode/literal labels, resize/live labels, all visual-row hit regions, disabled state and Enter/Space/pointer semantics. Noct tests actual stock-button adoption, config/CLI/faithful precedence and a neutral 40-column source dialog with long actions, hints, disabled action, mode switching, scroll/resize and owner dismissal. Count source IDs in Tab order and actions sent, not just strings in the model. Include large content beyond ordinary scroll's row cap and compact keyboard disclosure with F1 preserved. Capture neutral native screenshots in both modes and a reachable ordinary source popup without activating external consent/actions.

Engineering default ruling: wrapping in adaptive Noctiluca addresses the reported readability on first use; compact remains configurable. This intentionally changes Noctiluca's adaptive presentation, while preserving shared component defaults. Existing public primitives suffice for composition; the only selected new generic control capability is wrapped stock-button labels.
