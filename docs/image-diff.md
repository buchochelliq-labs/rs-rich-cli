# Comparing images

A rendering library needs a way to answer *"what changed?"* about its own output.
`scripts/image_diff.py` does that, and this page is both its documentation and its
test case.

The pair below is the same subject rendered twice. One has a halo. Everything
else is meant to be identical — but it isn't quite, and that turns out to be the
whole problem.

<div class="imgdiff"
     data-before="assets/diff/halo-before.webp"
     data-after="assets/diff/halo-after.webp"
     data-difference="assets/diff/halo-difference.webp"
     data-heatmap="assets/diff/halo-heatmap.webp"
     data-highlight="assets/diff/halo-highlight.webp"
     data-before-label="Before"
     data-after-label="After">
  <img src="assets/diff/halo-before.webp" alt="Before: neon elephant, no halo">
  <img src="assets/diff/halo-after.webp" alt="After: the same elephant with a golden halo">
</div>

## Why a pixel diff is not enough

The obvious implementation is `abs(before - after)`. On this pair it reports:

<div class="imgdiff-stats" markdown>

| | naive pixel diff | this tool |
|---|---|---|
| canvas reported changed | **42.2%** | **12.5%** |
| bounding box of change | 77% of the image | 14% of the image |
| top region | — | the halo, **47%** of all change |

</div>

Forty-two percent, with a box around three-quarters of the picture. Technically
true and completely useless — because these are two separate *generations*, not
one image with a halo pasted on. The elephant is redrawn with fractionally
different glow, line weight and position everywhere, and a pixel comparison
faithfully reports all of it.

Switch to the **Difference** tab above and you can see the problem directly: the
whole animal lights up. The halo is in there, but so is everything else.

## What the tool does instead

1. **Blur lightly** — high-frequency redraw noise cancels out; structural change
   survives.
2. **Compare in CIELAB**, not sRGB. A ΔE of 30 means roughly the same *perceived*
   difference wherever it falls, so a single threshold works across the whole
   image. In sRGB the same numeric delta is dramatic in shadow and invisible in
   highlight.
3. **Threshold, then morphologically open** — erode-then-dilate deletes isolated
   speckle while leaving coherent regions intact.
4. **Label connected components and rank them** by area × severity.

Step 4 is what makes the result readable. The answer stops being a cloud of
pixels and becomes *"three things changed, and here they are in order"*:

| rank | share of all change | mean ΔE | region |
|---|---|---|---|
| 1 | **47%** | 98.0 | `(277, 70)` 712×262 — **the halo** |
| 2 | 6% | 78.3 | `(605, 861)` 277×275 — the calf's trunk |
| 3 | 4% | 71.8 | `(172, 595)` 145×331 — the mother's trunk |

The halo is ranked first by roughly eight times. Try the **Highlight** tab: the
image dims to 35% and only the ranked regions stay lit.

## The modes, and what each is for

Different questions want different views, which is why the viewer has seven
rather than one.

| mode | answers |
|---|---|
| **Slider** | Did anything *move*? A wipe makes displacement obvious — edges jump as the handle crosses them. |
| **Onion** | How big is a *small* change? Cross-fading is better than wiping for sub-pixel shifts in weight or position. |
| **Blink** | Where should I even look? The eye is extremely good at catching flicker; this finds changes you would never spot side by side. |
| **Side by side** | What does each actually look like? Useful once you know where to look, useless for finding anything. |
| **Difference** | What changed at the pixel level, including noise? Familiar, and honest about texture the perceptual view suppresses. |
| **Heatmap** | How *perceptually* big is each change? Warmer is bigger. |
| **Highlight** | Just tell me what changed. The ranked answer. |

Blink respects `prefers-reduced-motion` and holds still if you have that set —
an automatic flicker is exactly what that setting asks to stop. The interval is
also deliberately slow, since fast blinking is both unreadable and a
photosensitivity hazard.

## Using it

```bash
python scripts/image_diff.py before.png after.png --out docs/assets/diff/name
```

It writes `name-before/-after/-difference/-heatmap/-highlight.webp` plus
`name.json` with the statistics and region list. The defaults are tuned for
*regenerated artwork*, which is the noisy end of the spectrum. Screenshot pairs
are far cleaner and want a lower threshold:

```bash
python scripts/image_diff.py a.png b.png --out out/x --blur 2 --threshold 20
```

Sizes must match. Comparing differently-sized images is meaningless, so the tool
refuses rather than silently rescaling and reporting nonsense.

!!! note "Why WebP"

    The outputs are WebP, not PNG. This content is glow over black — effectively
    photographic — so PNG's lossless compression has almost nothing to exploit
    and lands around 1 MB per image. At q=82 the difference is invisible here and
    the files are roughly ten times smaller, which matters on a page showing six
    of them at once.

## Where this is heading

Right now it is a manual tool. The obvious next step is wiring it into CI against
the generated [gallery](gallery.md) images, so a change that alters rendering
shows up as an annotated diff on the pull request rather than as a reviewer
noticing something looks off. That is tracked with the visual-regression work in
the [roadmap](ROADMAP.md).
