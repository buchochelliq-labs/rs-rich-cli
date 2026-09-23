# 0.0.4 baseline evidence

See [the benchmark method and targets](../../../docs/benchmarks.md#004-development-baseline).
`baseline.json` contains real process measurements against the exact 0.0.3 source
revision. The 5 MiB Markdown case was run separately with `--case wrap-markdown-5m`
and its timeout record appended; all other cases have five samples after warmup.
No runtime behavior changed in this baseline PR. The existing
[CLI notebook screenshot](../../../docs/assets/releases/0.0.3-notebook.jpg)
shows the unchanged 0.0.3 product; it is not evidence of a new feature.

Independent sub-agent review found no blockers in the sampler/harness.
Timeout race handling and explicit timeout metadata were added after review.
