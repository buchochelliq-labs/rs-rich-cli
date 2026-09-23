//! A small benchmark harness, a stable JSON run format and comparison.
//!
//! [`Bench::run`] warms up, then times batches of calls until a target
//! duration and a minimum sample count are both reached (or a maximum
//! count is). Calls too fast for the clock are batched so each sample spans
//! at least [`MIN_SAMPLE`]. Results are nanoseconds per call.
//!
//! # File format
//!
//! [`BenchRun`] serialises as JSON, schema version 1:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "created": "2026-09-23T12:00:00Z",
//!   "host": null,
//!   "measurements": [
//!     {"name": "table/80", "samples": 50, "mean": 1234.5, "median": 1200.0,
//!      "stddev": 40.2, "p95": 1300.0, "min": 1100.0, "max": 1500.0, "unit": "ns"}
//!   ]
//! }
//! ```
//!
//! [`BenchRun::from_criterion_dir`] reads criterion's output instead: every
//! `<dir>/**/new/estimates.json` (mean, median and std-dev point estimates)
//! with `new/sample.json` for the sample count, min, max and p95 when present.
//!
//! [`compare`] matches runs by name; [`ComparisonView`] renders the result
//! (HTML and SVG come from the console's export).

use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rich::{Console, ConsoleOptions, Renderable, Segment, Table, Text};
use serde::{Deserialize, Serialize};

use super::profile::format_nanos;
use super::{plural, table_then_line, Probe};

/// The shortest span one timed sample covers.
pub const MIN_SAMPLE: Duration = Duration::from_micros(20);

/// Statistics of one benchmark, in `unit` per call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub name: String,
    pub samples: usize,
    pub mean: f64,
    pub median: f64,
    pub stddev: f64,
    pub p95: f64,
    pub min: f64,
    pub max: f64,
    /// Always `ns` from this module.
    pub unit: String,
}

impl Measurement {
    /// Statistics of per-call nanosecond `samples` (sample stddev,
    /// nearest-rank p95).
    pub fn from_samples(name: impl Into<String>, samples: &[f64]) -> Self {
        let mut sorted: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        if n == 0 {
            return Measurement {
                name: name.into(),
                samples: 0,
                mean: 0.0,
                median: 0.0,
                stddev: 0.0,
                p95: 0.0,
                min: 0.0,
                max: 0.0,
                unit: "ns".into(),
            };
        }
        let mean = sorted.iter().sum::<f64>() / n as f64;
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        };
        let variance = if n > 1 {
            sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };
        let rank = ((n as f64) * 0.95).ceil() as usize;
        Measurement {
            name: name.into(),
            samples: n,
            mean,
            median,
            stddev: variance.sqrt(),
            p95: sorted[rank.clamp(1, n) - 1],
            min: sorted[0],
            max: sorted[n - 1],
            unit: "ns".into(),
        }
    }
}

/// A benchmark to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bench {
    pub name: String,
    /// Untimed running first (default 100 ms; at least one call).
    pub warmup: Duration,
    /// Keep sampling until this much time is spent (default 1 s)…
    pub target: Duration,
    /// …and at least this many samples are taken (default 10)…
    pub min_samples: usize,
    /// …but never more than this (default 10 000).
    pub max_samples: usize,
}

impl Bench {
    pub fn new(name: impl Into<String>) -> Self {
        Bench {
            name: name.into(),
            warmup: Duration::from_millis(100),
            target: Duration::from_secs(1),
            min_samples: 10,
            max_samples: 10_000,
        }
    }
    pub fn warmup(mut self, d: Duration) -> Self {
        self.warmup = d;
        self
    }
    pub fn target_time(mut self, d: Duration) -> Self {
        self.target = d;
        self
    }
    pub fn samples(mut self, min: usize, max: usize) -> Self {
        self.min_samples = min.max(1);
        self.max_samples = max.max(self.min_samples);
        self
    }

    /// Time `f` (see the [module docs](self)).
    pub fn run<T>(&self, mut f: impl FnMut() -> T) -> Measurement {
        let start = Instant::now();
        let mut calls = 0u32;
        loop {
            std::hint::black_box(f());
            calls += 1;
            if start.elapsed() >= self.warmup {
                break;
            }
        }
        let per_call = start.elapsed() / calls.max(1);
        let batch = if per_call.is_zero() {
            1000
        } else {
            (MIN_SAMPLE.as_nanos() / per_call.as_nanos().max(1)).clamp(1, 1_000_000) as u32
        };
        let mut samples = Vec::new();
        let begun = Instant::now();
        while samples.len() < self.max_samples
            && (samples.len() < self.min_samples || begun.elapsed() < self.target)
        {
            let t = Instant::now();
            for _ in 0..batch {
                std::hint::black_box(f());
            }
            samples.push(t.elapsed().as_nanos() as f64 / f64::from(batch));
        }
        Measurement::from_samples(self.name.clone(), &samples)
    }
}

/// Benchmark rendering `renderable` at `width` (segments and ANSI encoding,
/// truecolor, unicode) with default [`Bench`] settings.
pub fn bench_renderable(name: &str, renderable: &dyn Renderable, width: usize) -> Measurement {
    bench_renderable_with(&Bench::new(name), renderable, width)
}

/// [`bench_renderable`] with explicit settings.
pub fn bench_renderable_with(
    bench: &Bench,
    renderable: &dyn Renderable,
    width: usize,
) -> Measurement {
    let probe = Probe::new(width);
    let console = probe.target().console();
    let options = probe.options(&console);
    bench.run(|| {
        let segments = renderable.rich_render(&console, &options);
        console.segments_to_string(&segments)
    })
}

/// A set of measurements; the JSON file format (see the [module docs](self)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchRun {
    pub schema_version: u32,
    /// RFC 3339, UTC.
    pub created: String,
    pub host: Option<String>,
    pub measurements: Vec<Measurement>,
}

/// The schema version [`BenchRun`] writes.
pub const SCHEMA_VERSION: u32 = 1;

impl BenchRun {
    /// A run created now.
    pub fn new(measurements: Vec<Measurement>) -> Self {
        BenchRun {
            schema_version: SCHEMA_VERSION,
            created: rfc3339(SystemTime::now()),
            host: None,
            measurements,
        }
    }
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }
    pub fn get(&self, name: &str) -> Option<&Measurement> {
        self.measurements.iter().find(|m| m.name == name)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
    /// Parse a run; a newer schema version is an error.
    pub fn from_json(json: &str) -> io::Result<Self> {
        let run: BenchRun = serde_json::from_str(json).map_err(io::Error::other)?;
        if run.schema_version > SCHEMA_VERSION {
            return Err(io::Error::other(format!(
                "bench run schema {} is newer than {SCHEMA_VERSION}",
                run.schema_version
            )));
        }
        Ok(run)
    }
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, self.to_json() + "\n")
    }
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::from_json(&fs::read_to_string(path)?)
    }

    /// Read criterion results under `dir` (usually `target/criterion`); each
    /// benchmark is named by its path relative to `dir`, e.g. `group/case`.
    pub fn from_criterion_dir(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        let mut measurements = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current)? {
                let path = entry?.path();
                if !path.is_dir() {
                    continue;
                }
                let estimates = path.join("new").join("estimates.json");
                if path.file_name().is_some_and(|n| n != "new" && n != "base")
                    && estimates.is_file()
                {
                    let name = path
                        .strip_prefix(dir)
                        .unwrap_or(&path)
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    measurements.push(criterion_measurement(name, &path.join("new"))?);
                }
                if path.file_name().is_some_and(|n| n != "report") {
                    stack.push(path);
                }
            }
        }
        measurements.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(BenchRun::new(measurements))
    }
}

fn criterion_measurement(name: String, dir: &Path) -> io::Result<Measurement> {
    let read = |file: &str| -> io::Result<serde_json::Value> {
        serde_json::from_str(&fs::read_to_string(dir.join(file))?).map_err(io::Error::other)
    };
    let estimates = read("estimates.json")?;
    let point = |key: &str| {
        estimates
            .get(key)
            .and_then(|v| v.get("point_estimate"))
            .and_then(serde_json::Value::as_f64)
    };
    let mean = point("mean").ok_or_else(|| io::Error::other("estimates.json has no mean"))?;
    let mut m = Measurement {
        name,
        samples: 0,
        mean,
        median: point("median").unwrap_or(mean),
        stddev: point("std_dev").unwrap_or(0.0),
        p95: mean,
        min: mean,
        max: mean,
        unit: "ns".into(),
    };
    if let Ok(sample) = read("sample.json") {
        let list = |key: &str| -> Vec<f64> {
            sample
                .get(key)
                .and_then(serde_json::Value::as_array)
                .map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect())
                .unwrap_or_default()
        };
        let per_call: Vec<f64> = list("iters")
            .iter()
            .zip(list("times"))
            .filter(|(i, _)| **i > 0.0)
            .map(|(i, t)| t / i)
            .collect();
        if !per_call.is_empty() {
            let s = Measurement::from_samples(String::new(), &per_call);
            m.samples = s.samples;
            m.p95 = s.p95;
            m.min = s.min;
            m.max = s.max;
        }
    }
    Ok(m)
}

/// `time` as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn rfc3339(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Which statistic [`compare`] compares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Statistic {
    #[default]
    Mean,
    Median,
}

/// How noise is discounted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Noise {
    /// A change must also exceed the combined standard deviation
    /// `sqrt(sd_b² + sd_c²)`.
    #[default]
    Stddev,
    /// Only the threshold counts.
    Ignore,
}

/// [`compare`] settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompareOptions {
    /// Changes within ±this percentage are unchanged (default 5).
    pub threshold_pct: f64,
    pub noise: Noise,
    pub statistic: Statistic,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions {
            threshold_pct: 5.0,
            noise: Noise::Stddev,
            statistic: Statistic::Mean,
        }
    }
}

/// The outcome for one benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Regression,
    Improvement,
    Unchanged,
    New,
    Removed,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Regression => "regression",
            Verdict::Improvement => "improvement",
            Verdict::Unchanged => "unchanged",
            Verdict::New => "new",
            Verdict::Removed => "removed",
        }
    }
}

/// One benchmark in both runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub name: String,
    pub baseline: Option<Measurement>,
    pub candidate: Option<Measurement>,
    /// `(candidate - baseline) / baseline × 100`.
    pub change_pct: Option<f64>,
    pub verdict: Verdict,
}

/// The result of [`compare`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub rows: Vec<Row>,
}

impl Comparison {
    pub fn count(&self, verdict: Verdict) -> usize {
        self.rows.iter().filter(|r| r.verdict == verdict).count()
    }
    pub fn has_regressions(&self) -> bool {
        self.count(Verdict::Regression) > 0
    }
    pub fn row(&self, name: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.name == name)
    }
}

/// Compare `candidate` with `baseline`: baseline order, then new names.
pub fn compare(baseline: &BenchRun, candidate: &BenchRun, options: &CompareOptions) -> Comparison {
    let value = |m: &Measurement| match options.statistic {
        Statistic::Mean => m.mean,
        Statistic::Median => m.median,
    };
    let mut rows = Vec::new();
    for b in &baseline.measurements {
        let Some(c) = candidate.get(&b.name) else {
            rows.push(Row {
                name: b.name.clone(),
                baseline: Some(b.clone()),
                candidate: None,
                change_pct: None,
                verdict: Verdict::Removed,
            });
            continue;
        };
        let (bv, cv) = (value(b), value(c));
        let change = if bv > 0.0 {
            (cv - bv) / bv * 100.0
        } else {
            0.0
        };
        let noisy = match options.noise {
            Noise::Stddev => (cv - bv).abs() <= (b.stddev.powi(2) + c.stddev.powi(2)).sqrt(),
            Noise::Ignore => false,
        };
        let verdict = if noisy || change.abs() <= options.threshold_pct {
            Verdict::Unchanged
        } else if change > 0.0 {
            Verdict::Regression
        } else {
            Verdict::Improvement
        };
        rows.push(Row {
            name: b.name.clone(),
            baseline: Some(b.clone()),
            candidate: Some(c.clone()),
            change_pct: Some(change),
            verdict,
        });
    }
    for c in &candidate.measurements {
        if baseline.get(&c.name).is_none() {
            rows.push(Row {
                name: c.name.clone(),
                baseline: None,
                candidate: Some(c.clone()),
                change_pct: None,
                verdict: Verdict::New,
            });
        }
    }
    Comparison { rows }
}

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const ASCII_BLOCKS: [char; 8] = ['_', '.', '-', ':', '=', '+', '*', '#'];

/// A sparkline of `values` scaled to their maximum.
pub fn sparkline(values: &[f64], ascii: bool) -> String {
    let ramp = if ascii { &ASCII_BLOCKS } else { &BLOCKS };
    let max = values.iter().copied().fold(0.0_f64, f64::max);
    values
        .iter()
        .map(|&v| {
            if max <= 0.0 || !v.is_finite() {
                ramp[0]
            } else {
                ramp[((v / max) * 7.0).round().clamp(0.0, 7.0) as usize]
            }
        })
        .collect()
}

fn spread(m: &Measurement) -> [f64; 4] {
    [m.min, m.median, m.p95, m.max]
}

/// A [`Comparison`] as a table: a change column coloured by verdict, a
/// sparkline of min/median/p95/max for baseline then candidate, and a
/// summary line.
pub struct ComparisonView<'a> {
    comparison: &'a Comparison,
    ascii: Option<bool>,
}

impl<'a> ComparisonView<'a> {
    pub fn new(comparison: &'a Comparison) -> Self {
        ComparisonView {
            comparison,
            ascii: None,
        }
    }
    /// Force ASCII sparklines and units (default: the console's `ascii_only`).
    pub fn ascii(mut self, ascii: bool) -> Self {
        self.ascii = Some(ascii);
        self
    }
}

impl Renderable for ComparisonView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let ascii = self.ascii.unwrap_or_else(|| console.ascii_only());
        let c = self.comparison;
        let table = (!c.rows.is_empty()).then(|| {
            let mut table = Table::new();
            for header in [
                "Benchmark",
                "Baseline",
                "Candidate",
                "Change",
                "Spread",
                "Verdict",
            ] {
                table.add_column(header);
            }
            for row in &c.rows {
                let time = |m: &Option<Measurement>| {
                    m.as_ref()
                        .map_or("-".to_owned(), |m| format_nanos(m.mean, ascii))
                };
                let style = match row.verdict {
                    Verdict::Regression => "bold red",
                    Verdict::Improvement => "bold green",
                    Verdict::Unchanged => "dim",
                    Verdict::New | Verdict::Removed => "yellow",
                };
                let change = row
                    .change_pct
                    .map_or("-".to_owned(), |p| format!("{p:+.1}%"));
                let mut values = Vec::new();
                if let Some(b) = &row.baseline {
                    values.extend(spread(b));
                }
                let split = values.len();
                if let Some(cand) = &row.candidate {
                    values.extend(spread(cand));
                }
                let mut line = sparkline(&values, ascii);
                if split > 0 && split < values.len() {
                    line.insert(
                        line.char_indices()
                            .nth(split)
                            .map_or(line.len(), |(i, _)| i),
                        ' ',
                    );
                }
                table.add_row_text(vec![
                    Text::new(row.name.clone()),
                    Text::new(time(&row.baseline)),
                    Text::new(time(&row.candidate)),
                    Text::styled(change, style),
                    Text::new(line),
                    Text::styled(row.verdict.name(), style),
                ]);
            }
            table
        });
        let summary = format!(
            "{}: {}, {}, {} unchanged, {} new, {} removed",
            plural(c.rows.len(), "benchmark"),
            plural(c.count(Verdict::Regression), "regression"),
            plural(c.count(Verdict::Improvement), "improvement"),
            c.count(Verdict::Unchanged),
            c.count(Verdict::New),
            c.count(Verdict::Removed)
        );
        table_then_line(table, summary, console, options)
    }
}
