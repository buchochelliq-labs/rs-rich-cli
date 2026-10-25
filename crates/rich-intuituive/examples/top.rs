//! A system monitor in the style of [bottom](https://github.com/ClementTsang/bottom)
//! (`btm`), a ratatui app, rebuilt on intuiTUIve. It reuses none of
//! bottom's code: it is a rebuild of its default layout and keys. The graphs
//! are rich-ext's `Chart` (ratatui's `Chart`, with Braille lines) over a
//! sliding window of samples; the tables are `table_with`, sorted by the
//! app; the dialogs are modals.
//!
//! Ported from bottom 0.12 (<https://github.com/ClementTsang/bottom>), by
//! Clement Tsang and contributors, MIT licence. Its design, keys, layout and
//! help text are theirs; the credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example top             # the default layout
//!     cargo run -p rs-rich-intuituive --example top -- --basic  # basic mode
//!
//! Options, as bottom's: `-b`/`--basic`, `-r MS`/`--rate MS` (the sample
//! interval, 1000 by default), `-t MS`/`--default_time_value MS` (the
//! graphs' window, 60000 by default), `--disable_advanced_kill` (a yes/no
//! confirm rather than the list of signals).
//!
//! The data comes from Linux's `/proc` and `/sys` (and `df` for the disks'
//! sizes, `kill` to send a signal), behind the [`Source`] trait, so the
//! tests feed it samples of their own.
//!
//! On intuiTUIve's components: widgets are focus scopes moved between by
//! `bind_directional_focus`; expanding hides the others (`shown_when`);
//! the process table's titles follow the view (`columns_with`), its
//! columns drop by `priority`; the search is a `query` grammar; the sort
//! menu and the search box take the focus with `focus_within`.
//!
//! q quits · ? help · e expands the selected widget · ctrl or shift+arrows
//! (or H J K L, A S W D) move between widgets · + - = zoom the graphs ·
//! f freezes · ctrl+r resets · in the processes: c m p n sort, s the sort
//! menu, / or ctrl+f searches, t the tree, tab groups, dd or F9 kills.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use intuituive::interact::MouseKind;
use intuituive::prelude::*;
use intuituive::query::{
    self, Grammar, Kind as FieldKind, Matcher, QueryError, Record, TextMatch, Value,
};
use intuituive::reactive::NodeId;
use intuituive::rich::markup::escape;
use intuituive::rich::Style;
use intuituive::widgets::{
    table_with, tree_table_rows, Column, Order, TableLayout, TableOptions, TreeRow,
    TreeTableOptions,
};
use intuituive::Level;
use rich_ext::chart::{Axis, Chart, Dataset, GraphType, Labels, LegendPosition, Marker};

// ---------------------------------------------------------------------------
// The data: one sample of the whole machine, and where it comes from.
// ---------------------------------------------------------------------------

/// One sample of the machine, as bottom's harvest holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    /// Each core's use, in percent.
    pub cpus: Vec<f64>,
    /// The load averages over 1, 5 and 15 minutes.
    pub load: [f64; 3],
    pub ram: Mem,
    pub swap: Mem,
    pub net: Net,
    pub temps: Vec<Temp>,
    pub disks: Vec<Disk>,
    pub procs: Vec<Proc>,
}

impl Sample {
    /// The average over the cores.
    pub fn avg(&self) -> f64 {
        if self.cpus.is_empty() {
            0.0
        } else {
            self.cpus.iter().sum::<f64>() / self.cpus.len() as f64
        }
    }
}

/// Memory in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mem {
    pub used: u64,
    pub total: u64,
}

impl Mem {
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64 * 100.0
        }
    }
}

/// The network: rates in bits a second, totals in bits.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Net {
    pub rx: u64,
    pub tx: u64,
    pub total_rx: u64,
    pub total_tx: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Temp {
    pub name: String,
    pub celsius: f64,
}

/// A mounted disk: sizes in bytes, rates in bytes a second.
#[derive(Clone, Debug, PartialEq)]
pub struct Disk {
    pub name: String,
    pub mount: String,
    pub used: u64,
    pub free: u64,
    pub total: u64,
    pub read: u64,
    pub write: u64,
}

/// A process.
#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub command: String,
    /// Of the whole machine, in percent.
    pub cpu: f64,
    pub mem: u64,
    pub mem_percent: f64,
    /// Bytes a second, and in all.
    pub read: u64,
    pub write: u64,
    pub total_read: u64,
    pub total_write: u64,
    pub user: String,
    pub state: String,
    pub time: Duration,
    /// A kernel thread (no command line).
    pub kernel: bool,
}

/// Where samples come from, and where signals go: Linux's `/proc` in the
/// app, scripted samples in the tests.
pub trait Source: Send {
    /// The next sample. Rates are over the time since the last one.
    fn sample(&mut self) -> Sample;
    /// Send `signal` to `pid`.
    fn kill(&mut self, pid: u32, signal: i32) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// Linux, through std: /proc, /sys, `df` and `kill`.
// ---------------------------------------------------------------------------

/// The machine this runs on.
#[derive(Default)]
pub struct Linux {
    cores: Vec<(u64, u64)>,
    total: u64,
    procs: HashMap<u32, (u64, u64, u64)>,
    net: Option<(u64, u64)>,
    disks: HashMap<String, (u64, u64)>,
    last: Option<Instant>,
    users: HashMap<u32, String>,
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// A `/proc/stat` cpu line as (busy, total) jiffies.
fn jiffies(line: &str) -> (u64, u64) {
    let v: Vec<u64> = (line.split_whitespace().skip(1))
        .filter_map(|f| f.parse().ok())
        .collect();
    let total: u64 = v.iter().take(8).sum();
    let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
    (total - idle, total)
}

fn meminfo_kib(text: &str, key: &str) -> u64 {
    (text.lines())
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

impl Linux {
    pub fn new() -> Linux {
        let users = (read("/etc/passwd").lines())
            .filter_map(|l| {
                let f: Vec<&str> = l.split(':').collect();
                Some((f.get(2)?.parse().ok()?, f.first()?.to_string()))
            })
            .collect();
        Linux {
            users,
            ..Linux::default()
        }
    }

    fn temps() -> Vec<Temp> {
        let mut temps = Vec::new();
        let Ok(dirs) = std::fs::read_dir("/sys/class/hwmon") else {
            return temps;
        };
        let mut dirs: Vec<_> = dirs.flatten().map(|d| d.path()).collect();
        dirs.sort();
        for dir in dirs {
            let chip = std::fs::read_to_string(dir.join("name")).unwrap_or_default();
            let mut inputs: Vec<_> = (std::fs::read_dir(&dir).into_iter().flatten().flatten())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("temp") && n.ends_with("_input"))
                .collect();
            inputs.sort();
            for input in inputs {
                let Ok(milli) = std::fs::read_to_string(dir.join(&input))
                    .unwrap_or_default()
                    .trim()
                    .parse::<f64>()
                else {
                    continue;
                };
                let label = std::fs::read_to_string(dir.join(input.replace("_input", "_label")))
                    .unwrap_or_default();
                let name = match label.trim() {
                    "" => chip.trim().to_string(),
                    label => format!("{}: {label}", chip.trim()),
                };
                temps.push(Temp {
                    name,
                    celsius: milli / 1000.0,
                });
            }
        }
        temps
    }

    fn disks(&mut self, secs: f64) -> Vec<Disk> {
        let stats: HashMap<String, (u64, u64)> = (read("/proc/diskstats").lines())
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                let sectors = |i: usize| f.get(i)?.parse::<u64>().ok();
                Some((
                    f.get(2)?.to_string(),
                    (sectors(5)? * 512, sectors(9)? * 512),
                ))
            })
            .collect();
        let df = std::process::Command::new("df")
            .args(["-B1", "--output=source,target,used,avail,size"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        let mut disks = Vec::new();
        for line in df.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 5 || !f[0].starts_with("/dev/") || !seen.insert(f[0].to_string()) {
                continue;
            }
            let dev = f[0].trim_start_matches("/dev/").to_string();
            let (read, write) = stats.get(&dev).copied().unwrap_or((0, 0));
            let (last_read, last_write) = self
                .disks
                .insert(dev, (read, write))
                .unwrap_or((read, write));
            let rate = |now: u64, then: u64| (now.saturating_sub(then) as f64 / secs) as u64;
            disks.push(Disk {
                name: f[0].to_string(),
                mount: f[1].to_string(),
                used: f[2].parse().unwrap_or(0),
                free: f[3].parse().unwrap_or(0),
                total: f[4].parse().unwrap_or(0),
                read: rate(read, last_read),
                write: rate(write, last_write),
            });
        }
        disks
    }

    fn procs(&mut self, dtotal: u64, mem_total: u64, secs: f64) -> Vec<Proc> {
        let uptime: f64 = (read("/proc/uptime").split_whitespace().next())
            .and_then(|u| u.parse().ok())
            .unwrap_or(0.0);
        let mut seen = HashMap::new();
        let mut procs = Vec::new();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return procs;
        };
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let stat = read(&format!("/proc/{pid}/stat"));
            let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
                continue;
            };
            let name = stat[open + 1..close].to_string();
            let f: Vec<&str> = stat[close + 1..].split_whitespace().collect();
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let ticks = num(11) + num(12);
            let status = read(&format!("/proc/{pid}/status"));
            let field = |key: &str| {
                (status.lines())
                    .find(|l| l.starts_with(key))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string()
            };
            let uid: u32 = field("Uid:").parse().unwrap_or(0);
            let rss = field("VmRSS:").parse::<u64>().unwrap_or(0) * 1024;
            let io = read(&format!("/proc/{pid}/io"));
            let io_field = |key: &str| meminfo_kib(&io, key);
            let (total_read, total_write) = (io_field("read_bytes:"), io_field("write_bytes:"));
            let cmdline = read(&format!("/proc/{pid}/cmdline"));
            let command = cmdline.trim_end_matches('\0').replace('\0', " ");
            let (last_ticks, last_read, last_write) =
                (self.procs.get(&pid).copied()).unwrap_or((ticks, total_read, total_write));
            seen.insert(pid, (ticks, total_read, total_write));
            let state = match f.first().copied().unwrap_or("?") {
                "R" => "Running",
                "S" => "Sleeping",
                "D" => "Waiting",
                "Z" => "Zombie",
                "T" => "Stopped",
                "t" => "Tracing",
                "X" => "Dead",
                "I" => "Idle",
                _ => "Unknown",
            };
            let started = num(19) as f64 / 100.0;
            let rate = |now: u64, then: u64| (now.saturating_sub(then) as f64 / secs) as u64;
            procs.push(Proc {
                pid,
                ppid: num(1) as u32,
                command: if command.is_empty() {
                    name.clone()
                } else {
                    command
                },
                name,
                cpu: if dtotal == 0 {
                    0.0
                } else {
                    ticks.saturating_sub(last_ticks) as f64 / dtotal as f64 * 100.0
                },
                mem: rss,
                mem_percent: if mem_total == 0 {
                    0.0
                } else {
                    rss as f64 / mem_total as f64 * 100.0
                },
                read: rate(total_read, last_read),
                write: rate(total_write, last_write),
                total_read,
                total_write,
                user: self
                    .users
                    .get(&uid)
                    .cloned()
                    .unwrap_or_else(|| uid.to_string()),
                state: state.to_string(),
                time: Duration::from_secs_f64((uptime - started).max(0.0)),
                kernel: cmdline.is_empty(),
            });
        }
        self.procs = seen;
        procs
    }
}

impl Source for Linux {
    fn sample(&mut self) -> Sample {
        let now = Instant::now();
        let secs = self
            .last
            .map_or(1.0, |l| now.duration_since(l).as_secs_f64().max(0.001));
        self.last = Some(now);

        let stat = read("/proc/stat");
        let mut lines = stat.lines().filter(|l| l.starts_with("cpu"));
        let (_, total) = lines.next().map(jiffies).unwrap_or((0, 0));
        let dtotal = total.saturating_sub(self.total);
        self.total = total;
        let cores: Vec<(u64, u64)> = lines.map(jiffies).collect();
        let cpus = (cores.iter().enumerate())
            .map(|(i, &(busy, total))| {
                let (b0, t0) = self.cores.get(i).copied().unwrap_or((busy, total));
                let dt = total.saturating_sub(t0);
                if dt == 0 {
                    0.0
                } else {
                    busy.saturating_sub(b0) as f64 / dt as f64 * 100.0
                }
            })
            .collect();
        self.cores = cores;

        let loadavg = read("/proc/loadavg");
        let mut load = [0.0; 3];
        for (slot, v) in load.iter_mut().zip(loadavg.split_whitespace()) {
            *slot = v.parse().unwrap_or(0.0);
        }

        let mem = read("/proc/meminfo");
        let kib = |key| meminfo_kib(&mem, key) * 1024;
        let ram = Mem {
            used: kib("MemTotal:").saturating_sub(kib("MemAvailable:")),
            total: kib("MemTotal:"),
        };
        let swap = Mem {
            used: kib("SwapTotal:").saturating_sub(kib("SwapFree:")),
            total: kib("SwapTotal:"),
        };

        let (mut rx, mut tx) = (0u64, 0u64);
        for line in read("/proc/net/dev").lines().skip(2) {
            let Some((iface, rest)) = line.split_once(':') else {
                continue;
            };
            if iface.trim() == "lo" {
                continue;
            }
            let f: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            rx += f.first().copied().unwrap_or(0);
            tx += f.get(8).copied().unwrap_or(0);
        }
        let (rx0, tx0) = self.net.replace((rx, tx)).unwrap_or((rx, tx));
        let net = Net {
            rx: (rx.saturating_sub(rx0) as f64 * 8.0 / secs) as u64,
            tx: (tx.saturating_sub(tx0) as f64 * 8.0 / secs) as u64,
            total_rx: rx * 8,
            total_tx: tx * 8,
        };

        Sample {
            cpus,
            load,
            ram,
            swap,
            net,
            temps: Linux::temps(),
            disks: self.disks(secs),
            procs: self.procs(dtotal, ram.total, secs),
        }
    }

    fn kill(&mut self, pid: u32, signal: i32) -> Result<(), String> {
        let out = std::process::Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(pid.to_string())
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Units, as bottom says them.
// ---------------------------------------------------------------------------

const KI: f64 = 1024.0;
const K: f64 = 1000.0;

/// A size in 1024s: `(value, "GiB")`.
pub fn binary_bytes(bytes: u64) -> (f64, &'static str) {
    let b = bytes as f64;
    match b {
        b if b < KI => (b, "B"),
        b if b < KI * KI => (b / KI, "KiB"),
        b if b < KI * KI * KI => (b / (KI * KI), "MiB"),
        b if b < KI * KI * KI * KI => (b / (KI * KI * KI), "GiB"),
        b => (b / (KI * KI * KI * KI), "TiB"),
    }
}

/// A size in 1000s: `(value, "GB")`.
pub fn decimal_bytes(bytes: u64) -> (f64, &'static str) {
    let b = bytes as f64;
    match b {
        b if b < K => (b, "B"),
        b if b < K * K => (b / K, "KB"),
        b if b < K * K * K => (b / (K * K), "MB"),
        b if b < K * K * K * K => (b / (K * K * K), "GB"),
        b => (b / (K * K * K * K), "TB"),
    }
}

/// A unit prefix in 1000s: `(value, "K")`.
pub fn decimal_prefix(value: u64) -> (f64, &'static str) {
    let (v, unit) = decimal_bytes(value);
    (v, unit.trim_end_matches('B'))
}

/// bottom's process cells: a decimal place from a giga on.
pub fn binary_string(bytes: u64) -> String {
    let (v, unit) = binary_bytes(bytes);
    if bytes as f64 >= KI * KI * KI {
        format!("{v:.1}{unit}")
    } else {
        format!("{v:.0}{unit}")
    }
}

pub fn decimal_string(bytes: u64) -> String {
    let (v, unit) = decimal_bytes(bytes);
    if bytes as f64 >= K * K * K {
        format!("{v:.1}{unit}")
    } else {
        format!("{v:.0}{unit}")
    }
}

pub fn per_second(bytes: u64) -> String {
    format!("{}/s", decimal_string(bytes))
}

/// A process's run time, as bottom shows it.
pub fn format_time(d: Duration) -> String {
    let s = d.as_secs();
    let (days, hours, minutes) = (s / 86_400, s / 3600, s / 60);
    if days > 0 {
        format!("{days}d {}h {}m", hours % 24, minutes % 60)
    } else if hours > 0 {
        format!("{hours}h {}m {}s", minutes % 60, s % 60)
    } else if minutes > 0 {
        format!("{minutes}m {}.{:02}s", s % 60, d.subsec_millis() / 10)
    } else {
        format!("{s}.{:03}s", d.subsec_millis())
    }
}

/// The memory legend: `RAM: 23%   3.6GiB/15.5GiB`.
pub fn memory_label(name: &str, mem: &Mem) -> String {
    if mem.total == 0 {
        return format!("{name}:   0%   0.0B/0.0B");
    }
    let (total, unit) = binary_bytes(mem.total);
    let used = total * mem.used as f64 / mem.total as f64;
    format!(
        "{name}:{:3.0}%   {used:.1}{unit}/{total:.1}{unit}",
        mem.percent()
    )
}

/// The network graph's top and its labels for the largest rate in view
/// (bits a second, decimal prefixes): bottom's linear scale.
pub fn network_axis(max: f64) -> (f64, Vec<String>) {
    let upper = if max == 0.0 { 1.0 } else { max * 1.5 };
    let (scaled, prefix) = match upper {
        u if u < K => (max, ""),
        u if u < K * K => (max / K, "K"),
        u if u < K * K * K => (max / (K * K), "M"),
        u if u < K * K * K * K => (max / (K * K * K), "G"),
        _ => (max / (K * K * K * K), "T"),
    };
    let labels = [
        format!("0{prefix}b"),
        format!("{:.1}", scaled * 0.5),
        format!("{scaled:.1}"),
        format!("{:.1}", scaled * 1.5),
    ];
    (upper, labels.iter().map(|s| format!("{s:>5}")).collect())
}

/// The network legend's rate: `1.2Kb/s`.
pub fn bits_label(bits: u64) -> String {
    let (v, prefix) = decimal_prefix(bits);
    format!("{v:.1}{prefix}b/s")
}

// ---------------------------------------------------------------------------
// The history the graphs show: a sliding window over the samples.
// ---------------------------------------------------------------------------

/// The graphs' points of one sample, at `t` ms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Point {
    pub t: u64,
    pub avg: f64,
    pub cpus: Vec<f64>,
    pub ram: f64,
    pub swap: f64,
    pub rx: f64,
    pub tx: f64,
}

/// What the graphs show, kept for bottom's retention (ten minutes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub points: VecDeque<Point>,
    pub ticks: u64,
}

/// bottom's limits on the window: 30 s to the retention, 15 s a step.
pub const MIN_WINDOW: u64 = 30_000;
pub const RETENTION: u64 = 600_000;
pub const ZOOM_STEP: u64 = 15_000;

impl History {
    pub fn push(&mut self, s: &Sample, interval: u64) {
        let t = self.ticks * interval;
        self.ticks += 1;
        self.points.push_back(Point {
            t,
            avg: s.avg(),
            cpus: s.cpus.clone(),
            ram: s.ram.percent(),
            swap: s.swap.percent(),
            rx: s.net.rx as f64,
            tx: s.net.tx as f64,
        });
        while self
            .points
            .front()
            .is_some_and(|p| p.t + RETENTION + interval < t)
        {
            self.points.pop_front();
        }
    }

    /// One series over the last `window` ms, as x from `-window` to 0 (and
    /// one point before, so the line runs into the left edge).
    pub fn series(&self, window: u64, value: impl Fn(&Point) -> f64) -> Vec<(f64, f64)> {
        let Some(now) = self.points.back().map(|p| p.t) else {
            return Vec::new();
        };
        let from = self
            .points
            .iter()
            .rposition(|p| p.t + window < now)
            .unwrap_or(0);
        (self.points.iter().skip(from))
            .map(|p| (p.t as f64 - now as f64, value(p)))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// The processes: search, sort, group and tree, as bottom's table shows them.
// ---------------------------------------------------------------------------

pub const PROC_COLUMNS: [&str; 11] = [
    "PID", "Name", "CPU%", "Mem%", "R/s", "W/s", "T.Read", "T.Write", "User", "State", "Time",
];

/// How the process table is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcView {
    pub sort: Option<(usize, Order)>,
    pub tree: bool,
    pub grouped: bool,
    pub command: bool,
    pub mem_bytes: bool,
    pub hide_kernel: bool,
}

impl Default for ProcView {
    fn default() -> Self {
        ProcView {
            sort: Some((2, Order::Descending)),
            tree: false,
            grouped: false,
            command: false,
            mem_bytes: false,
            hide_kernel: false,
        }
    }
}

impl ProcView {
    /// The column titles: PID becomes Count when grouped, Name Command, and
    /// Mem% Mem.
    pub fn titles(&self) -> Vec<String> {
        let mut titles: Vec<String> = PROC_COLUMNS.iter().map(|t| t.to_string()).collect();
        if self.grouped {
            titles[0] = "Count".into();
        }
        if self.command {
            titles[1] = "Command".into();
        }
        if self.mem_bytes {
            titles[3] = "Mem".into();
        }
        titles
    }
}

/// The search: bottom's query, with its case and whole-word options. The
/// regex option is shown and toggles, but no regex engine is among the
/// crate's dependencies, so a pattern is matched as plain text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub text: String,
    pub case: bool,
    pub whole: bool,
    pub regex: bool,
}

/// A process as the search reads it: its fields by bottom's keywords,
/// and its name (or with `P`, its command) as its text.
struct Searched<'a>(&'a Proc, bool);

impl Record for Searched<'_> {
    fn field(&self, name: &str) -> Option<Value> {
        let p = self.0;
        Some(match name {
            "pid" => Value::Number(p.pid as f64),
            "cpu" => Value::Number(p.cpu),
            "mem" => Value::Number(p.mem_percent),
            "memb" => Value::Number(p.mem as f64),
            "read" => Value::Number(p.read as f64),
            "write" => Value::Number(p.write as f64),
            "user" => Value::from(p.user.as_str()),
            "state" => Value::from(p.state.as_str()),
            _ => return None,
        })
    }

    fn text(&self) -> String {
        match self.1 {
            true => self.0.command.clone(),
            false => self.0.name.clone(),
        }
    }
}

fn unit_bytes(value: f64, unit: &str) -> f64 {
    let scale = match unit.to_lowercase().as_str() {
        "kb" => K,
        "mb" => K * K,
        "gb" => K * K * K,
        "tb" => K * K * K * K,
        "kib" => KI,
        "mib" => KI * KI,
        "gib" => KI * KI * KI,
        "tib" => KI * KI * KI * KI,
        _ => 1.0,
    };
    value * scale
}

/// A number of bytes, with a unit or without: `1gib`, `500kb`, `4096`.
fn bytes(text: &str) -> Option<Value> {
    let at = text.find(|c: char| c.is_alphabetic()).unwrap_or(text.len());
    let (number, unit) = text.split_at(at);
    let number: f64 = number.parse().ok()?;
    (unit.is_empty() || unit_bytes(1.0, unit) != 1.0)
        .then(|| Value::Number(unit_bytes(number, unit)))
}

/// Whether `word` is a whole word of `text`.
fn whole_word(text: &str, word: &str, case: bool) -> bool {
    let fold = |t: &str| {
        if case {
            t.to_string()
        } else {
            t.to_lowercase()
        }
    };
    let (text, word) = (fold(text), fold(word));
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .any(|w| w == word)
}

impl Query {
    /// bottom's search as a grammar: its keywords as fields, sizes with
    /// units, and bare words matching the name (whole words with Alt+W).
    fn grammar(&self) -> Grammar {
        let grammar = Grammar::new()
            .field("pid", FieldKind::Number)
            .field("cpu", FieldKind::Number)
            .alias("cpu%", "cpu")
            .field("mem", FieldKind::Number)
            .alias("mem%", "mem")
            .field("memb", FieldKind::Number)
            .read(bytes)
            .field("read", FieldKind::Number)
            .read(bytes)
            .alias("r/s", "read")
            .alias("rps", "read")
            .field("write", FieldKind::Number)
            .read(bytes)
            .alias("w/s", "write")
            .alias("wps", "write")
            .field("user", FieldKind::Text)
            .matching(TextMatch::Contains)
            .field("state", FieldKind::Text)
            .matching(TextMatch::Contains)
            .case_sensitive(self.case)
            .without_tags();
        if !self.whole {
            return grammar;
        }
        let case = self.case;
        grammar.words(move |word| {
            let word = word.to_string();
            let matcher: Matcher =
                Rc::new(move |p: &dyn Record| whole_word(&p.text(), &word, case));
            Some(matcher)
        })
    }

    /// The query read, a unit after a number joined to it (`1 gib`) and
    /// `&&` and `||` read as `and` and `or`.
    fn parsed(&self) -> Result<query::Query, QueryError> {
        let mut words: Vec<String> = Vec::new();
        for word in self.text.split_whitespace() {
            match words.last_mut() {
                Some(last) if unit_bytes(1.0, word) != 1.0 && last.parse::<f64>().is_ok() => {
                    last.push_str(word)
                }
                _ => words.push(match word {
                    "&&" => "and".into(),
                    "||" => "or".into(),
                    word => word.into(),
                }),
            }
        }
        query::parse(&words.join(" "), &self.grammar())
    }

    /// Whether `p` is shown: every process when the query has an error,
    /// as bottom does.
    pub fn matches(&self, p: &Proc, command: bool) -> bool {
        self.parsed()
            .map_or(true, |q| q.matches(&Searched(p, command)))
    }

    pub fn error(&self) -> Option<String> {
        self.parsed().err().map(|e| e.message)
    }
}

/// A row of the process table: the processes it stands for (several when
/// grouped) and its cells.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcRow {
    pub pids: Vec<u32>,
    pub name: String,
    pub cells: Vec<String>,
}

fn compare(a: &Proc, b: &Proc, column: usize, command: bool) -> std::cmp::Ordering {
    let by = |x: f64, y: f64| x.total_cmp(&y);
    match column {
        0 => a.pid.cmp(&b.pid),
        1 if command => a.command.to_lowercase().cmp(&b.command.to_lowercase()),
        1 => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        2 => by(a.cpu, b.cpu),
        3 => a.mem.cmp(&b.mem),
        4 => a.read.cmp(&b.read),
        5 => a.write.cmp(&b.write),
        6 => a.total_read.cmp(&b.total_read),
        7 => a.total_write.cmp(&b.total_write),
        8 => a.user.cmp(&b.user),
        9 => a.state.cmp(&b.state),
        _ => a.time.cmp(&b.time),
    }
    .then(a.pid.cmp(&b.pid))
}

fn sort_procs(procs: &mut [Proc], view: &ProcView) {
    if let Some((column, order)) = view.sort {
        procs.sort_by(|a, b| {
            let o = compare(a, b, column, view.command);
            if order == Order::Descending {
                o.reverse()
            } else {
                o
            }
        });
    }
}

fn cells(p: &Proc, view: &ProcView, first: String) -> Vec<String> {
    let name = if view.command { &p.command } else { &p.name };
    let mem = if view.mem_bytes {
        binary_string(p.mem)
    } else {
        format!("{:.1}%", p.mem_percent)
    };
    vec![
        first,
        escape(name),
        format!("{:.1}%", p.cpu),
        mem,
        per_second(p.read),
        per_second(p.write),
        decimal_string(p.total_read),
        decimal_string(p.total_write),
        if view.grouped {
            String::new()
        } else {
            escape(&p.user)
        },
        if view.grouped {
            String::new()
        } else {
            p.state.clone()
        },
        format_time(p.time),
    ]
}

/// The table's rows: filtered by the query, then sorted, grouped by name,
/// or laid out as a tree (children under their parent, sorted among
/// themselves; `collapsed` parents show `+` and hide their children).
pub fn proc_rows(
    procs: &[Proc],
    view: &ProcView,
    query: &Query,
    collapsed: &BTreeSet<u32>,
) -> Vec<ProcRow> {
    let kept = |p: &Proc| !(view.hide_kernel && p.kernel);
    let query = query.parsed().ok();
    let found = |p: &Proc| {
        query
            .as_ref()
            .is_none_or(|q| q.matches(&Searched(p, view.command)))
    };
    let shown: Vec<Proc> = (procs.iter())
        .filter(|p| kept(p) && found(p))
        .cloned()
        .collect();
    if view.tree {
        return tree_rows(procs, &shown, view, collapsed);
    }
    if view.grouped {
        let mut groups: BTreeMap<String, (Proc, Vec<u32>)> = BTreeMap::new();
        for p in shown {
            let key = if view.command {
                p.command.clone()
            } else {
                p.name.clone()
            };
            match groups.get_mut(&key) {
                Some((sum, pids)) => {
                    sum.cpu += p.cpu;
                    sum.mem += p.mem;
                    sum.mem_percent += p.mem_percent;
                    sum.read += p.read;
                    sum.write += p.write;
                    sum.total_read += p.total_read;
                    sum.total_write += p.total_write;
                    sum.time = sum.time.max(p.time);
                    pids.push(p.pid);
                }
                None => {
                    let pid = p.pid;
                    groups.insert(key, (p, vec![pid]));
                }
            }
        }
        let mut sums: Vec<(Proc, Vec<u32>)> = groups.into_values().collect();
        let mut summed: Vec<Proc> = sums.iter().map(|(p, _)| p.clone()).collect();
        sort_procs(&mut summed, view);
        return summed
            .into_iter()
            .map(|p| {
                let at = sums
                    .iter()
                    .position(|(s, _)| s.pid == p.pid)
                    .expect("a group");
                let (_, pids) = sums.swap_remove(at);
                ProcRow {
                    cells: cells(&p, view, pids.len().to_string()),
                    name: p.name.clone(),
                    pids,
                }
            })
            .collect();
    }
    let mut shown = shown;
    sort_procs(&mut shown, view);
    (shown.iter())
        .map(|p| ProcRow {
            pids: vec![p.pid],
            name: p.name.clone(),
            cells: cells(p, view, p.pid.to_string()),
        })
        .collect()
}

fn tree_rows(
    all: &[Proc],
    shown: &[Proc],
    view: &ProcView,
    collapsed: &BTreeSet<u32>,
) -> Vec<ProcRow> {
    // A process is kept when it matches, or a descendant does.
    let by_pid: HashMap<u32, &Proc> = all.iter().map(|p| (p.pid, p)).collect();
    let mut kept: BTreeSet<u32> = BTreeSet::new();
    for p in shown {
        let mut at = Some(p.pid);
        while let Some(pid) = at {
            if !kept.insert(pid) {
                break;
            }
            at = by_pid
                .get(&pid)
                .map(|p| p.ppid)
                .filter(|pp| by_pid.contains_key(pp));
        }
    }
    let mut children: BTreeMap<u32, Vec<Proc>> = BTreeMap::new();
    let mut roots = Vec::new();
    for p in all.iter().filter(|p| kept.contains(&p.pid)) {
        if kept.contains(&p.ppid) && p.ppid != p.pid {
            children.entry(p.ppid).or_default().push(p.clone());
        } else {
            roots.push(p.clone());
        }
    }
    sort_procs(&mut roots, view);
    for list in children.values_mut() {
        sort_procs(list, view);
    }
    // The tree, drawn as a tree table draws it: bottom's guides, and `+`
    // on a collapsed parent.
    fn row(p: &Proc, children: &BTreeMap<u32, Vec<Proc>>, view: &ProcView) -> TreeRow<u32> {
        let kids = children.get(&p.pid).map(Vec::as_slice).unwrap_or(&[]);
        TreeRow::new(p.pid, cells(p, view, p.pid.to_string()))
            .children(kids.iter().map(|kid| row(kid, children, view)))
    }
    let tree: Vec<TreeRow<u32>> = roots.iter().map(|p| row(p, &children, view)).collect();
    let options = TreeTableOptions::new().tree_column(1);
    tree_table_rows(&tree, |pid| collapsed.contains(pid), &options)
        .into_iter()
        .map(|shown| ProcRow {
            pids: vec![shown.key],
            name: by_pid
                .get(&shown.key)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            cells: shown.cells,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The temperature and disk tables.
// ---------------------------------------------------------------------------

pub fn temp_rows(temps: &[Temp], sort: Option<(usize, Order)>) -> Vec<Vec<String>> {
    let mut temps = temps.to_vec();
    if let Some((column, order)) = sort {
        temps.sort_by(|a, b| {
            let o = match column {
                0 => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                _ => a.celsius.total_cmp(&b.celsius),
            };
            if order == Order::Descending {
                o.reverse()
            } else {
                o
            }
        });
    }
    (temps.iter())
        .map(|t| vec![escape(&t.name), format!("{:.0}°C", t.celsius)])
        .collect()
}

pub const DISK_COLUMNS: [&str; 8] = [
    "Disk", "Mount", "Used", "Free", "Total", "Used%", "R/s", "W/s",
];

pub fn disk_rows(disks: &[Disk], sort: Option<(usize, Order)>) -> Vec<Vec<String>> {
    let mut disks = disks.to_vec();
    let percent = |d: &Disk| {
        if d.total == 0 {
            0.0
        } else {
            d.used as f64 / d.total as f64 * 100.0
        }
    };
    if let Some((column, order)) = sort {
        disks.sort_by(|a, b| {
            let o = match column {
                0 => a.name.cmp(&b.name),
                1 => a.mount.cmp(&b.mount),
                2 => a.used.cmp(&b.used),
                3 => a.free.cmp(&b.free),
                4 => a.total.cmp(&b.total),
                5 => percent(a).total_cmp(&percent(b)),
                6 => a.read.cmp(&b.read),
                _ => a.write.cmp(&b.write),
            };
            if order == Order::Descending {
                o.reverse()
            } else {
                o
            }
        });
    }
    (disks.iter())
        .map(|d| {
            vec![
                escape(&d.name),
                escape(&d.mount),
                binary_string(d.used),
                binary_string(d.free),
                binary_string(d.total),
                format!("{:.1}%", percent(d)),
                per_second(d.read),
                per_second(d.write),
            ]
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The app.
// ---------------------------------------------------------------------------

/// bottom's command line, the parts this rebuild takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub basic: bool,
    /// The sample interval, ms.
    pub rate: u64,
    /// The graphs' window, ms.
    pub default_time: u64,
    pub advanced_kill: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            basic: false,
            rate: 1000,
            default_time: 60_000,
            advanced_kill: true,
        }
    }
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut number = |name: &str| -> Result<u64, String> {
            (args.next())
                .and_then(|v| v.parse().ok())
                .ok_or(format!("{name} takes a number of milliseconds"))
        };
        match arg.as_str() {
            "-b" | "--basic" => options.basic = true,
            "-r" | "--rate" => options.rate = number(arg)?.max(250),
            "-t" | "--default_time_value" => {
                options.default_time = number(arg)?.clamp(MIN_WINDOW, RETENTION)
            }
            "--disable_advanced_kill" => options.advanced_kill = false,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// The widgets of the default layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Cpu,
    CpuLegend,
    Mem,
    Temp,
    Disk,
    Net,
    Proc,
}

/// bottom's default theme, as console styles.
const CPU_COLOURS: [&str; 8] = [
    "bright_magenta",
    "bright_yellow",
    "bright_cyan",
    "bright_green",
    "bright_blue",
    "cyan",
    "green",
    "blue",
];
const AVG_COLOUR: &str = "red";
const ALL_COLOUR: &str = "green";
const RAM_COLOUR: &str = "bright_magenta";
const SWAP_COLOUR: &str = "bright_yellow";
const RX_COLOUR: &str = "bright_magenta";
const TX_COLOUR: &str = "bright_yellow";
const TEXT: &str = "white";
const HIGHLIGHT: &str = "bright_blue";

/// The state the widgets share: every widget is a view of these signals.
#[derive(Clone, Copy)]
struct State {
    options: Signal<Options>,
    shown: Signal<Sample>,
    history: Signal<History>,
    frozen: Signal<bool>,
    expanded: Signal<Option<Kind>>,
    cpu_window: Signal<u64>,
    mem_window: Signal<u64>,
    net_window: Signal<u64>,
    cpu_selected: Signal<usize>,
    /// How the processes show; their sort is `proc_sort`, the table's.
    proc_view: Signal<ProcView>,
    proc_sort: Signal<Option<(usize, Order)>>,
    proc_selected: Signal<usize>,
    collapsed: Signal<BTreeSet<u32>>,
    query: Signal<String>,
    query_options: Signal<(bool, bool, bool)>,
    search_open: Signal<bool>,
    sort_open: Signal<bool>,
    sort_selected: Signal<usize>,
    temp_sort: Signal<Option<(usize, Order)>>,
    temp_selected: Signal<usize>,
    disk_sort: Signal<Option<(usize, Order)>>,
    disk_selected: Signal<usize>,
    rows: Memo<Vec<ProcRow>>,
    basic_table: Signal<Kind>,
}

impl State {
    fn query(&self) -> Query {
        let (case, whole, regex) = self.query_options.get();
        Query {
            text: self.query.get(),
            case,
            whole,
            regex,
        }
    }

    /// Whether a widget of `kinds` shows: all do, unless another is
    /// expanded.
    fn shows(self, kinds: &'static [Kind]) -> impl Fn() -> bool {
        move || self.expanded.get().is_none_or(|k| kinds.contains(&k))
    }
}

/// The keys every widget of the dashboard takes: expanding, and, on a
/// graph, zooming its window. Each widget is a focus scope, so the
/// dashboard's directional keys move between widgets.
fn widget_keys(node: Node, state: State, kind: Kind, window: Option<Signal<u64>>) -> Node {
    // Basic mode has no expanding, and its arrows move between tables.
    if state.options.get_untracked().basic {
        return node;
    }
    let node = node.focus_scope().bind(
        "e",
        "Toggle expanding the currently selected widget",
        move |_| {
            state.expanded.update(|e| {
                *e = match e {
                    Some(_) => None,
                    None => Some(kind),
                }
            })
        },
    );
    match window {
        Some(window) => {
            let zoom_in =
                move || window.update(|w| *w = w.saturating_sub(ZOOM_STEP).max(MIN_WINDOW));
            let zoom_out = move || window.update(|w| *w = (*w + ZOOM_STEP).min(RETENTION));
            node.bind(
                "plus",
                "Zoom in on chart (decrease time range)",
                move |_| zoom_in(),
            )
            .bind(
                "minus",
                "Zoom out on chart (increase time range)",
                move |_| zoom_out(),
            )
            .bind("equals", "Reset zoom", move |_| {
                window.set(state.options.get_untracked().default_time)
            })
        }
        None => node,
    }
}

/// The wheel over a graph zooms it, as bottom's does: up in, down out.
fn wheel_zoom(graph: Node, window: Signal<u64>) -> Node {
    graph.on_mouse(move |_, mouse| {
        let by = match mouse.kind {
            MouseKind::ScrollUp => -(ZOOM_STEP as i64),
            MouseKind::ScrollDown => ZOOM_STEP as i64,
            _ => return false,
        };
        window.update(|w| *w = (*w as i64 + by).clamp(MIN_WINDOW as i64, RETENTION as i64) as u64);
        true
    })
}

/// A graph's x axis: `60s` to `0s`.
fn time_axis(window: u64) -> Axis {
    Axis::default()
        .bounds(-(window as f64), 0.0)
        .labels(Labels::Text(vec![
            format!("{}s", window / 1000),
            "0s".into(),
        ]))
        .style(TEXT)
}

fn percent_axis() -> Axis {
    Axis::default()
        .bounds(0.0, 100.5)
        .labels(Labels::Text(vec!["  0%".into(), "100%".into()]))
        .style(TEXT)
}

fn line(points: Vec<(f64, f64)>, style: &str) -> Dataset {
    Dataset::new(points)
        .graph_type(GraphType::Line)
        .marker(Marker::Braille)
        .style(style)
}

/// The CPU graph: every core and the average while the legend's `All` is
/// selected, else the one selected.
fn cpu_chart(state: State) -> Chart {
    let window = state.cpu_window.get();
    let selected = state.cpu_selected.get();
    let datasets = state.history.with(|h| {
        let cores = h.points.back().map_or(0, |p| p.cpus.len());
        let avg = || line(h.series(window, |p| p.avg), AVG_COLOUR);
        let core = |i: usize| {
            line(
                h.series(window, |p| p.cpus.get(i).copied().unwrap_or(0.0)),
                CPU_COLOURS[i % CPU_COLOURS.len()],
            )
        };
        match selected {
            0 => {
                let mut all: Vec<Dataset> = (0..cores).rev().map(core).collect();
                all.push(avg());
                all
            }
            1 => vec![avg()],
            n => vec![core(n - 2)],
        }
    });
    Chart::new(datasets)
        .marker(Marker::Braille)
        .x_axis(time_axis(window))
        .y_axis(percent_axis())
        .legend(LegendPosition::None)
}

fn mem_chart(state: State) -> Chart {
    let window = state.mem_window.get();
    let (ram, swap) = state.shown.with(|s| (s.ram, s.swap));
    let mut datasets = Vec::new();
    state.history.with(|h| {
        datasets
            .push(line(h.series(window, |p| p.ram), RAM_COLOUR).name(memory_label("RAM", &ram)));
        if swap.total > 0 {
            datasets.push(
                line(h.series(window, |p| p.swap), SWAP_COLOUR).name(memory_label("SWP", &swap)),
            );
        }
    });
    Chart::new(datasets)
        .marker(Marker::Braille)
        .x_axis(time_axis(window))
        .y_axis(percent_axis())
        .legend(LegendPosition::TopRight)
        .hidden_legend_fraction(0.75, 0.75)
        .legend_style(TEXT)
}

fn net_chart(state: State) -> Chart {
    let window = state.net_window.get();
    let net = state.shown.with(|s| s.net);
    let (rx, tx) = state
        .history
        .with(|h| (h.series(window, |p| p.rx), h.series(window, |p| p.tx)));
    let max = (rx.iter().chain(&tx)).map(|p| p.1).fold(0.0, f64::max);
    let (upper, labels) = network_axis(max);
    let total = |bits: u64| {
        let (v, unit) = decimal_bytes(bits / 8);
        format!("{v:.1}{unit}")
    };
    let rx_name = format!(
        "RX: {:<10} All: {}",
        bits_label(net.rx),
        total(net.total_rx)
    );
    let tx_name = format!(
        "TX: {:<10} All: {}",
        bits_label(net.tx),
        total(net.total_tx)
    );
    Chart::new([
        line(rx, RX_COLOUR).name(rx_name),
        line(tx, TX_COLOUR).name(tx_name),
    ])
    .marker(Marker::Braille)
    .x_axis(time_axis(window))
    .y_axis(
        Axis::default()
            .bounds(0.0, upper)
            .labels(Labels::Text(labels))
            .style(TEXT),
    )
    .legend(LegendPosition::TopRight)
    .hidden_legend_fraction(0.9, 0.75)
    .legend_style(TEXT)
}

/// A table's sort key: the same column again reverses it, another starts
/// in its own default order.
fn sort_by(sort: Signal<Option<(usize, Order)>>, column: usize, default: Order) {
    sort.update(|s| {
        *s = match *s {
            Some((c, order)) if c == column => Some((
                column,
                if order == Order::Ascending {
                    Order::Descending
                } else {
                    Order::Ascending
                },
            )),
            _ => Some((column, default)),
        }
    })
}

fn proc_default_order(column: usize) -> Order {
    match column {
        0 | 1 | 8 | 9 => Order::Ascending,
        _ => Order::Descending,
    }
}

fn proc_sort(state: State, column: usize) {
    sort_by(state.proc_sort, column, proc_default_order(column))
}

/// The panel every widget sits in: bottom's plain border, its title at
/// the left and, expanded, `Esc to go back` at the right.
fn boxed(node: Node, title: &str, kind: Kind, state: State) -> Node {
    node.panel(title)
        .border(Border::Plain)
        .add_title_with(Edge::Top, Align::End, move || {
            match state.expanded.get() == Some(kind) {
                true => "Esc to go back".into(),
                false => String::new(),
            }
        })
}

/// The CPU graph, its title with the load averages.
fn cpu_graph(state: State) -> Node {
    let graph = renderable(move || cpu_chart(state)).focusable().name("cpu");
    let graph = wheel_zoom(graph, state.cpu_window);
    let node = boxed(graph, "CPU", Kind::Cpu, state).title_with(move || {
        let [one, five, fifteen] = state.shown.with(|s| s.load);
        format!("CPU ─ {one:.2} {five:.2} {fifteen:.2}")
    });
    widget_keys(node, state, Kind::Cpu, Some(state.cpu_window))
}

/// The CPU legend: `All`, `AVG`, then each core, with its use.
fn cpu_legend(state: State) -> Node {
    let columns = vec![
        Column::new("CPU", Size::Auto),
        Column::new("Use", Size::Auto).align(Align::End),
    ];
    let rows = move || {
        state.shown.with(|s| {
            let mut rows = vec![vec!["All".to_string(), String::new()]];
            rows.push(vec!["AVG".into(), format!("{:.0}%", s.avg())]);
            for (i, u) in s.cpus.iter().enumerate() {
                rows.push(vec![format!("CPU{i}"), format!("{u:.0}%")]);
            }
            rows
        })
    };
    let options = TableOptions::default().row_style(|i| {
        Some(match i {
            0 => ALL_COLOUR.to_string(),
            1 => AVG_COLOUR.to_string(),
            n => CPU_COLOURS[(n - 2) % CPU_COLOURS.len()].to_string(),
        })
    });
    let table = table_with(columns, rows, state.cpu_selected, options).name("cpu legend");
    let node = boxed(table, "", Kind::CpuLegend, state);
    widget_keys(node, state, Kind::CpuLegend, None)
}

fn mem_graph(state: State) -> Node {
    let graph = renderable(move || mem_chart(state))
        .focusable()
        .name("memory");
    let graph = wheel_zoom(graph, state.mem_window);
    let node = boxed(graph, "Memory", Kind::Mem, state);
    widget_keys(node, state, Kind::Mem, Some(state.mem_window))
}

fn net_graph(state: State) -> Node {
    let graph = renderable(move || net_chart(state))
        .focusable()
        .name("network");
    let graph = wheel_zoom(graph, state.net_window);
    let node = boxed(graph, "Network", Kind::Net, state);
    widget_keys(node, state, Kind::Net, Some(state.net_window))
}

fn temp_table(state: State) -> Node {
    let columns = vec![
        Column::new("Sensor", Size::Flex(1)),
        Column::new("Temp", Size::Auto).align(Align::End),
    ];
    let table = table_with(
        columns,
        move || {
            state
                .shown
                .with(|s| temp_rows(&s.temps, state.temp_sort.get()))
        },
        state.temp_selected,
        TableOptions::default().sort(state.temp_sort),
    )
    .name("temperatures");
    let keys = column([table])
        .previews_keys()
        .bind(
            "s",
            "Sort by sensor name, press again to reverse",
            move |_| sort_by(state.temp_sort, 0, Order::Ascending),
        )
        .bind(
            "t",
            "Sort by temperature, press again to reverse",
            move |_| sort_by(state.temp_sort, 1, Order::Descending),
        );
    let node = boxed(keys, "Temperatures", Kind::Temp, state);
    widget_keys(node, state, Kind::Temp, None)
}

fn disk_table(state: State) -> Node {
    let columns = DISK_COLUMNS
        .iter()
        .enumerate()
        .map(|(i, t)| {
            // The rightmost go first when they do not fit, as bottom's.
            let c = Column::new(t, Size::Auto);
            if i >= 2 {
                c.align(Align::End).priority((DISK_COLUMNS.len() - i) as u8)
            } else {
                c.priority(u8::MAX)
            }
        })
        .collect();
    let table = table_with(
        columns,
        move || {
            state
                .shown
                .with(|s| disk_rows(&s.disks, state.disk_sort.get()))
        },
        state.disk_selected,
        TableOptions::default().sort(state.disk_sort),
    )
    .name("disks");
    let mut keys = column([table]).previews_keys();
    let what = [
        ("d", "disk name", Order::Ascending),
        ("m", "disk mount", Order::Ascending),
        ("u", "disk usage", Order::Descending),
        ("n", "disk free space", Order::Descending),
        ("t", "total disk space", Order::Descending),
        ("p", "disk usage percentage", Order::Descending),
        ("r", "disk read activity", Order::Descending),
        ("w", "disk write activity", Order::Descending),
    ];
    for (column, (key, name, order)) in what.into_iter().enumerate() {
        let description = format!("Sort by {name}, press again to reverse");
        keys = keys.bind(key, &description, move |_| {
            sort_by(state.disk_sort, column, order)
        });
    }
    let node = boxed(keys, "Disks", Kind::Disk, state);
    widget_keys(node, state, Kind::Disk, None)
}

/// bottom's signals on Linux, for the kill dialog.
pub fn signals() -> Vec<(i32, String)> {
    let names = [
        "Cancel", "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "KILL", "USR1",
        "SEGV", "USR2", "PIPE", "ALRM", "TERM", "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN",
        "TTOU", "URG", "XCPU", "XFSZ", "VTALRM", "PROF", "WINCH", "IO", "PWR", "SYS",
    ];
    let mut all: Vec<(i32, String)> = (names.iter().enumerate())
        .map(|(i, n)| (i as i32, n.to_string()))
        .collect();
    all.push((34, "RTMIN".into()));
    all.extend((1..=15).map(|n| (34 + n, format!("RTMIN+{n}"))));
    all.extend((1..=14).rev().map(|n| (64 - n, format!("RTMAX-{n}"))));
    all.push((64, "RTMAX".into()));
    all
}

/// Send `signal` to each of `pids`; a failure is a notification, as
/// bottom's error dialog.
fn send(cx: &mut Ctx, source: &Arc<Mutex<Box<dyn Source>>>, pids: &[u32], name: &str, signal: i32) {
    for pid in pids {
        let result = source
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut s| s.kill(*pid, signal));
        if let Err(why) = result {
            cx.notify(
                Level::Error,
                "Error",
                format!(
                    "Failed to kill process {} ({pid}):\n{}",
                    escape(name),
                    escape(&why)
                ),
            );
            return;
        }
    }
}

/// bottom's kill dialog: the process, then the signals with TERM picked.
fn kill_dialog(cx: &mut Ctx, state: State, source: Arc<Mutex<Box<dyn Source>>>) {
    let rows = state.rows.get_untracked();
    let Some(row) = rows.get(state.proc_selected.get_untracked()).cloned() else {
        return;
    };
    let short: String = row.name.chars().take(20).collect();
    let text = if row.pids.len() > 1 {
        format!(
            "Kill {} processes with the name '{}'? Press ENTER to confirm.",
            row.pids.len(),
            escape(&short)
        )
    } else {
        format!(
            "Kill process '{}' with PID {}? Press ENTER to confirm.",
            escape(&short),
            row.pids[0]
        )
    };
    if !state.options.get_untracked().advanced_kill {
        cx.confirm("Confirm Kill Process", text, Vec::new(), move |cx| {
            send(cx, &source, &row.pids, &row.name, 15)
        });
        return;
    }
    let all = signals();
    let term = all.iter().position(|(n, _)| *n == 15).unwrap_or(0);
    cx.modal(Size::Fixed(60), Size::Percent(70), move || {
        let picked = signal(term);
        let names: Vec<String> = all.iter().map(|(n, s)| format!("{n}: {s}")).collect();
        let pick = list(move || names.clone(), picked)
            .autofocus()
            .name("signals");
        column([
            label(text).text_align(TextAlign::Center).auto(),
            label("").fixed(1),
            pick.flex(1),
        ])
        .panel("Select Signal")
        .border(Border::Plain)
        .title_right("Esc to close")
        .on_key("enter", move |cx| {
            let signal = all[picked.get_untracked()].0;
            cx.pop();
            if signal != 0 {
                send(cx, &source, &row.pids, &row.name, signal);
            }
        })
        .on_key("esc", |cx| cx.pop())
    });
}

/// bottom's help, in its sections.
pub const HELP: &str = "\
1 - General
q, Q, Ctrl-c            Quit
Esc                     Close dialog windows, search, widgets, or exit expanded mode
Ctrl-r                  Reset display and any collected data
f                       Freeze/unfreeze updating with new data
Shift/Ctrl-Left, H, A   Move widget selection left
Shift/Ctrl-Right, L, D  Move widget selection right
Shift/Ctrl-Up, K, W     Move widget selection up
Shift/Ctrl-Down, J, S   Move widget selection down
Left, h                 Move left within widget
Down, j                 Move down within widget
Up, k                   Move up within widget
Right, l                Move right within widget
?                       Open help menu
gg                      Jump to the first entry
G                       Jump to the last entry
e                       Toggle expanding the currently selected widget
+                       Zoom in on chart (decrease time range)
-                       Zoom out on chart (increase time range)
=                       Reset zoom
PgUp, PgDown            Scroll up/down a table by a page
Ctrl-u, Ctrl-d          Scroll up/down a table by half a page
Mouse click             Selects the clicked widget, table entry, dialog option, or tab

2 - CPU widget
Up, Down                Select a CPU thread/average to show in the graph

3 - Process widget
dd, F9, Delete          Kill the selected process
c                       Sort by CPU usage, press again to reverse
m                       Sort by memory usage, press again to reverse
p                       Sort by PID name, press again to reverse
n                       Sort by process name, press again to reverse
Tab                     Group/un-group processes with the same name
Ctrl-f, /               Open process search widget
P                       Toggle between showing the full command or just the process name
s, F6                   Open process sort widget
I                       Invert current sort
%                       Toggle between values and percentages for memory usage
t, F5                   Toggle tree mode
Left                    Collapse a branch while in tree mode
Right                   Expand a branch while in tree mode
+, -, Space             Toggle whether a branch is expanded or collapsed in tree mode
click on header         Sorts the entries by that column, click again to invert the sort
z                       Toggle the display of kernel threads

4 - Process search widget
Esc                     Close the search widget (retains the filter)
Alt-c, F1               Toggle matching case
Alt-w, F2               Toggle matching the entire word
Alt-r, F3               Toggle using regex

Supported search types:
<by name/cmd>           ex: btm
pid                     ex: pid 825
cpu, cpu%               ex: cpu > 4.2
mem, mem%               ex: mem < 4.2
memb                    ex: memb < 100 kb
read, r/s, rps          ex: read >= 1 b
write, w/s, wps         ex: write <= 1 tb
user                    ex: user = root
state                   ex: state = running

5 - Sort widget
Down, 'j'               Scroll down in list
Up, 'k'                 Scroll up in list
Esc                     Close the sort widget
Enter                   Sort by current selected column

6 - Temperature widget
's'                     Sort by sensor name, press again to reverse
't'                     Sort by temperature, press again to reverse

7 - Disk widget
'd'                     Sort by disk name, press again to reverse
'm'                     Sort by disk mount, press again to reverse
'u'                     Sort by disk usage, press again to reverse
'n'                     Sort by disk free space, press again to reverse
't'                     Sort by total disk space, press again to reverse
'p'                     Sort by disk usage percentage, press again to reverse
'r'                     Sort by disk read activity, press again to reverse
'w'                     Sort by disk write activity, press again to reverse";

fn help(cx: &mut Ctx) {
    cx.modal(Size::Percent(80), Size::Percent(80), || {
        scroll(label(escape(HELP)))
            .autofocus()
            .name("help")
            .panel("Help")
            .border(Border::Plain)
            .title_right("Esc to close")
            .on_key("esc", |cx| cx.pop())
            .on_key("q Q", |cx| cx.quit())
    });
}

/// The process table: its columns' titles follow the view (PID becomes
/// Count when grouped, Name Command, Mem% Mem), and the rightmost go
/// first when they do not fit, as bottom's do.
fn proc_table(state: State, layout: &TableLayout) -> Node {
    let columns = move || {
        let titles = state.proc_view.with(|v| v.titles());
        (titles.iter().enumerate())
            .map(|(i, t)| {
                let c = match i {
                    0 => Column::new(t, Size::Auto).priority(u8::MAX),
                    1 => Column::new(t, Size::Flex(1)).min(16).priority(u8::MAX),
                    i => Column::new(t, Size::Auto).priority((PROC_COLUMNS.len() - i) as u8),
                };
                if (2..8).contains(&i) {
                    c.align(Align::End)
                } else {
                    c
                }
            })
            .collect()
    };
    let rows = move || {
        state
            .rows
            .with(|rows| rows.iter().map(|r| r.cells.clone()).collect())
    };
    let options = TableOptions::default()
        .columns_with(columns)
        .sort(state.proc_sort)
        .layout(layout);
    let table = table_with(Vec::new(), rows, state.proc_selected, options).name("processes");
    // bottom starts on the processes.
    if state.options.get_untracked().basic {
        table
    } else {
        table.autofocus()
    }
}

/// The sort menu at the left of the processes: `s` or F6. Closing it
/// gives the focus back to `table`.
fn sort_menu(state: State, table: NodeId) -> Node {
    let names = move || state.proc_view.with(|v| v.titles());
    let close = move |cx: &mut Ctx| {
        state.sort_open.set(false);
        cx.focus(table);
    };
    list(names, state.sort_selected)
        .panel("Sort By")
        .border(Border::Plain)
        .on_key("esc", close)
        .on_key("enter", move |cx| {
            let column = state.sort_selected.get_untracked();
            state
                .proc_sort
                .set(Some((column, proc_default_order(column))));
            close(cx);
        })
        // bottom's is 11 wide, its title against the corner; a panel's
        // title sits a glyph in, so two more.
        .fixed(13)
        .shown_when(move || state.sort_open.get())
}

/// The search box under the processes: `/` or ctrl+f. Closing it gives
/// the focus back to `table`.
fn search_box(state: State, table: NodeId) -> Node {
    let field = input(state.query).name("search");
    let options = text(move || {
        let (case, whole, regex) = state.query_options.get();
        let on = |b: bool, t: &str| {
            if b {
                format!("[black on {HIGHLIGHT}]{t}[/]")
            } else {
                t.to_string()
            }
        };
        format!(
            "{}  {}  {}",
            on(case, "Case(Alt+C)"),
            on(whole, "Whole(Alt+W)"),
            on(regex, "Regex(Alt+R)")
        )
    });
    let error = text(move || match state.query().error() {
        Some(e) => format!("[red]{}[/]", escape(&e)),
        None => String::new(),
    });
    let toggle = |which: usize| {
        move |_: &mut Ctx| {
            state.query_options.update(|o| match which {
                0 => o.0 = !o.0,
                1 => o.1 = !o.1,
                _ => o.2 = !o.2,
            })
        }
    };
    column([
        row([label(format!("[bold {HIGHLIGHT}]> [/]")).fixed(2), field]).fixed(1),
        error.fixed(1),
        options.fixed(1),
    ])
    .panel("")
    .border(Border::Plain)
    .title_right("Esc to close")
    .on_key("alt+c f1", toggle(0))
    .on_key("alt+w f2", toggle(1))
    .on_key("alt+r f3", toggle(2))
    .on_key("ctrl+u", move |_| state.query.set(String::new()))
    .on_key("esc", move |cx| {
        state.search_open.set(false);
        cx.focus(table);
    })
    .fixed(5)
    .shown_when(move || state.search_open.get())
}

/// The process widget: the table, the sort menu at its left and the
/// search under it, with the table's keys.
fn proc_widget(state: State, source: Arc<Mutex<Box<dyn Source>>>) -> Node {
    let layout = TableLayout::new();
    let table = proc_table(state, &layout);
    let table_id = table.id();
    let menu = sort_menu(state, table_id);
    let menu_id = menu.id();
    let search = search_box(state, table_id);
    let search_id = search.id();
    let half_page = move |sign: i64| {
        let layout = layout.clone();
        move |_: &mut Ctx| {
            let rows = (layout.rows_in_view() / 2).max(1) as i64;
            let len = state.rows.with_untracked(Vec::len) as i64;
            state
                .proc_selected
                .update(|s| *s = (*s as i64 + sign * rows).clamp(0, (len - 1).max(0)) as usize);
        }
    };
    let tree = move || state.proc_view.with(|v| v.tree);
    let toggle_branch = move |open: Option<bool>| {
        move |_: &mut Ctx| {
            let rows = state.rows.get_untracked();
            let Some(pid) = rows
                .get(state.proc_selected.get_untracked())
                .map(|r| r.pids[0])
            else {
                return;
            };
            state.collapsed.update(|c| {
                let shut = match open {
                    Some(open) => !open,
                    None => !c.contains(&pid),
                };
                if shut {
                    c.insert(pid);
                } else {
                    c.remove(&pid);
                }
            });
        }
    };
    let open_search = move |cx: &mut Ctx| {
        state.search_open.set(true);
        cx.focus_within(search_id);
    };
    let open_sort = move |cx: &mut Ctx| {
        let at = state.proc_sort.with_untracked(|s| s.map_or(0, |s| s.0));
        state.sort_selected.set(at);
        state.sort_open.set(true);
        cx.focus_within(menu_id);
    };
    let kill = {
        let source = source.clone();
        move |cx: &mut Ctx| kill_dialog(cx, state, source.clone())
    };
    let kill_seq = kill.clone();
    let view = state.proc_view;
    let keyed = column([table])
        .previews_keys()
        .bind(
            "c",
            "Sort by CPU usage, press again to reverse",
            move |_| proc_sort(state, 2),
        )
        .bind(
            "m",
            "Sort by memory usage, press again to reverse",
            move |_| proc_sort(state, 3),
        )
        .bind("p", "Sort by PID name, press again to reverse", move |_| {
            proc_sort(state, 0)
        })
        .bind(
            "n",
            "Sort by process name, press again to reverse",
            move |_| proc_sort(state, 1),
        )
        .bind("I", "Invert current sort", move |_| {
            state.proc_sort.update(|s| {
                if let Some((_, order)) = s {
                    *order = match order {
                        Order::Ascending => Order::Descending,
                        Order::Descending => Order::Ascending,
                    };
                }
            })
        })
        .bind(
            "tab",
            "Group/un-group processes with the same name",
            move |_| {
                view.update(|v| {
                    if !v.tree {
                        v.grouped = !v.grouped
                    }
                })
            },
        )
        .bind(
            "P",
            "Toggle between showing the full command or just the process name",
            move |_| view.update(|v| v.command = !v.command),
        )
        .bind(
            "percent",
            "Toggle between values and percentages for memory usage",
            move |_| view.update(|v| v.mem_bytes = !v.mem_bytes),
        )
        .bind("t f5", "Toggle tree mode", move |_| {
            view.update(|v| {
                v.tree = !v.tree;
                v.grouped = false;
            })
        })
        .bind("z", "Toggle the display of kernel threads", move |_| {
            view.update(|v| v.hide_kernel = !v.hide_kernel)
        })
        .bind("ctrl+f slash", "Open process search widget", open_search)
        .bind("s f6", "Open process sort widget", open_sort)
        .bind("f9 delete", "Kill the selected process", kill)
        .bind_sequence("d d", "Kill the selected process", kill_seq)
        .on_key("ctrl+u", half_page(-1))
        .on_key("ctrl+d", half_page(1))
        .when(tree, |node| {
            node.bind(
                "left",
                "Collapse a branch while in tree mode",
                toggle_branch(Some(false)),
            )
            .bind(
                "right",
                "Expand a branch while in tree mode",
                toggle_branch(Some(true)),
            )
            .bind(
                "plus minus space",
                "Toggle whether a branch is expanded",
                toggle_branch(None),
            )
        });
    let body = row([menu, keyed.flex(1)]);
    let node = boxed(body, "Processes", Kind::Proc, state);
    let node = column([node.flex(1), search]);
    widget_keys(node, state, Kind::Proc, None)
}

/// One widget by its kind.
fn widget_of(kind: Kind, state: State, source: &Arc<Mutex<Box<dyn Source>>>) -> Node {
    match kind {
        Kind::Cpu | Kind::CpuLegend => {
            row([cpu_graph(state).flex(1), cpu_legend(state).percent(15)])
        }
        Kind::Mem => mem_graph(state),
        Kind::Temp => temp_table(state),
        Kind::Disk => disk_table(state),
        Kind::Net => net_graph(state),
        Kind::Proc => proc_widget(state, source.clone()),
    }
}

/// bottom's default layout: CPU on top, memory beside temperatures over
/// disks, then network beside processes, in rows of 30, 40 and 30. An
/// expanded widget hides the others, and so takes the screen.
fn dashboard(state: State, source: &Arc<Mutex<Box<dyn Source>>>) -> Node {
    let w = |kind: Kind, kinds: &'static [Kind]| {
        widget_of(kind, state, source).shown_when(state.shows(kinds))
    };
    use Kind::*;
    column([
        w(Cpu, &[Cpu, CpuLegend]).flex(30),
        row([
            w(Mem, &[Mem]).flex(4),
            column([w(Temp, &[Temp]), w(Disk, &[Disk])])
                .flex(3)
                .shown_when(state.shows(&[Temp, Disk])),
        ])
        .flex(40)
        .shown_when(state.shows(&[Mem, Temp, Disk])),
        row([w(Net, &[Net]).flex(1), w(Proc, &[Proc]).flex(1)])
            .flex(30)
            .shown_when(state.shows(&[Net, Proc])),
    ])
    .bind_directional_focus(
        "ctrl+left shift+left H A",
        "ctrl+right shift+right L D",
        "ctrl+up shift+up K W",
        "ctrl+down shift+down J S",
    )
}

/// An htop-style pipe gauge, as bottom's basic mode draws them:
/// `CPU0 [|||||      12%]`. A gauge that knows its width would do this.
pub fn pipe_gauge(label: &str, ratio: f64, inner: &str, width: usize, style: &str) -> String {
    let inside = width.saturating_sub(label.chars().count() + 3);
    let filled = ((ratio.clamp(0.0, 1.0)) * inside as f64).round() as usize;
    let inner_len = inner.chars().count().min(inside);
    let bars = filled.min(inside - inner_len);
    let gap = inside - inner_len - bars;
    format!(
        "{label} [[{style}]{}[/]{}{}]",
        "|".repeat(bars),
        " ".repeat(gap),
        escape(inner)
    )
}

/// bottom's basic mode: gauges instead of graphs, and one table at a time.
fn basic(state: State, source: &Arc<Mutex<Box<dyn Source>>>) -> Node {
    let cpus = text(move || {
        let width = viewport().get().0 as usize;
        let per_row = if width >= 120 {
            4
        } else if width >= 60 {
            2
        } else {
            1
        };
        let cell = (width.saturating_sub(2) / per_row).saturating_sub(1);
        state.shown.with(|s| {
            let mut gauges: Vec<String> = vec![pipe_gauge(
                "AVG",
                s.avg() / 100.0,
                &format!("{:>3.0}%", s.avg()),
                cell,
                AVG_COLOUR,
            )];
            for (i, u) in s.cpus.iter().enumerate() {
                gauges.push(pipe_gauge(
                    &format!("{i:<3}"),
                    u / 100.0,
                    &format!("{u:>3.0}%"),
                    cell,
                    CPU_COLOURS[i % CPU_COLOURS.len()],
                ));
            }
            gauges
                .chunks(per_row)
                .map(|c| c.join(" "))
                .collect::<Vec<_>>()
                .join("\n")
        })
    });
    let mem = text(move || {
        let width = viewport().get().0 as usize / 2;
        state.shown.with(|s| {
            let label = |m: &Mem| {
                let (total, unit) = binary_bytes(m.total);
                let used = if m.total == 0 {
                    0.0
                } else {
                    total * m.used as f64 / m.total as f64
                };
                format!("{used:.1}{unit}/{total:.1}{unit}")
            };
            let mut lines = vec![pipe_gauge(
                "RAM",
                s.ram.percent() / 100.0,
                &label(&s.ram),
                width - 2,
                RAM_COLOUR,
            )];
            if s.swap.total > 0 {
                lines.push(pipe_gauge(
                    "SWP",
                    s.swap.percent() / 100.0,
                    &label(&s.swap),
                    width - 2,
                    SWAP_COLOUR,
                ));
            }
            lines.join("\n")
        })
    });
    let net = text(move || {
        state.shown.with(|s| {
            let total = |bits: u64| {
                let (v, unit) = decimal_bytes(bits / 8);
                format!("{v:.1}{unit}")
            };
            format!(
                "[{RX_COLOUR}]RX: {:<10}[/] [{TX_COLOUR}]TX: {}[/]\n[{RX_COLOUR}]Total RX: {:<4}[/] [{TX_COLOUR}]Total TX: {}[/]",
                bits_label(s.net.rx),
                bits_label(s.net.tx),
                total(s.net.total_rx),
                total(s.net.total_tx),
            )
        })
    });
    let order = [Kind::Proc, Kind::Temp, Kind::Disk];
    let step = move |by: isize| {
        move |_: &mut Ctx| {
            let at = order
                .iter()
                .position(|k| *k == state.basic_table.get_untracked())
                .unwrap_or(0);
            let next = (at as isize + by).clamp(0, order.len() as isize - 1) as usize;
            state.basic_table.set(order[next]);
        }
    };
    let name = |k: Kind| match k {
        Kind::Temp => "Temperatures",
        Kind::Disk => "Disks",
        _ => "Processes",
    };
    let at = move || {
        order
            .iter()
            .position(|k| *k == state.basic_table.get())
            .unwrap_or(0)
    };
    let arrows = row([
        text(move || {
            at().checked_sub(1)
                .map(|i| format!("◄ {}", name(order[i])))
                .unwrap_or_default()
        }),
        text(move || {
            order
                .get(at() + 1)
                .map(|k| format!("{} ►", name(*k)))
                .unwrap_or_default()
        })
        .auto(),
    ])
    .padding(0, 1);
    let source = source.clone();
    let tables = switch(
        move || state.basic_table.get(),
        move |kind| match kind {
            Kind::Temp => temp_table(state),
            Kind::Disk => disk_table(state),
            _ => proc_widget(state, source.clone()),
        },
    );
    column([
        cpus.auto().padding(0, 1),
        label("").fixed(1),
        row([mem.flex(1), net.flex(1)]).auto().padding(0, 1),
        label("").fixed(1),
        arrows.fixed(1),
        tables.flex(1),
    ])
    .bind("ctrl+left shift+left H A", "Previous table", step(-1))
    .bind("ctrl+right shift+right L D", "Next table", step(1))
}

/// The app, sampling `source` every `options.rate` ms on a thread of its own.
pub fn top_app(options: Options, source: Box<dyn Source>) -> App {
    let source: Arc<Mutex<Box<dyn Source>>> = Arc::new(Mutex::new(source));
    App::new(move || {
        let options_signal = signal(options.clone());
        let shown = signal(Sample::default());
        let proc_view = signal(ProcView::default());
        let collapsed = signal(BTreeSet::new());
        let query = signal(String::new());
        let query_options = signal((false, false, false));
        let proc_sort = signal(ProcView::default().sort);
        let rows = memo(move || {
            let (case, whole, regex) = query_options.get();
            let q = Query {
                text: query.get(),
                case,
                whole,
                regex,
            };
            let view = ProcView {
                sort: proc_sort.get(),
                ..proc_view.get()
            };
            collapsed.with(|c| shown.with(|s| proc_rows(&s.procs, &view, &q, c)))
        });
        let state = State {
            options: options_signal,
            shown,
            history: signal(History::default()),
            frozen: signal(false),
            expanded: signal(None),
            cpu_window: signal(options.default_time),
            mem_window: signal(options.default_time),
            net_window: signal(options.default_time),
            cpu_selected: signal(0),
            proc_view,
            proc_sort,
            proc_selected: signal(0),
            collapsed,
            query,
            query_options,
            search_open: signal(false),
            sort_open: signal(false),
            sort_selected: signal(0),
            temp_sort: signal(None),
            temp_selected: signal(0),
            disk_sort: signal(None),
            disk_selected: signal(0),
            rows,
            basic_table: signal(Kind::Proc),
        };

        // The samples, each on the thread; the app keeps a history of them.
        let sampler = source.clone();
        let latest = latest_every(Duration::from_millis(options.rate), move || {
            sampler.lock().map(|mut s| s.sample()).unwrap_or_default()
        });
        let interval = options.rate;
        on_write(latest, move |sample, _| {
            if let Some(sample) = sample {
                if !state.frozen.get_untracked() {
                    state.history.update(|h| h.push(&sample, interval));
                    state.shown.set(sample);
                }
            }
        });

        let body = if options.basic {
            basic(state, &source)
        } else {
            dashboard(state, &source)
        };
        let frozen = label(format!(
            "[black on {HIGHLIGHT}]Frozen, press 'f' to unfreeze[/]"
        ))
        .padding(0, 1)
        .fixed(1)
        .shown_when(move || state.frozen.get());
        let back = move |_: &mut Ctx| state.expanded.set(None);
        column([body.flex(1), frozen])
            .bind("q Q", "Quit", |cx| cx.quit())
            .bind("question", "Open help menu", help)
            .bind("f", "Freeze/unfreeze updating with new data", move |_| {
                state.frozen.update(|f| *f = !*f)
            })
            .bind(
                "ctrl+r",
                "Reset display and any collected data",
                move |_| state.history.set(History::default()),
            )
            .bind("esc", "Exit expanded mode", back)
    })
    .theme(
        Theme::dark()
            .style("border", Style::parse(TEXT).expect("a style"))
            .style("border.focused", Style::parse(HIGHLIGHT).expect("a style"))
            .style("title", Style::parse(TEXT).expect("a style"))
            .style(
                "selected",
                Style::parse(&format!("black on {HIGHLIGHT}")).expect("a style"),
            )
            .style(
                "table.selected",
                Style::parse("not reverse").expect("a style"),
            )
            .style(
                "table.header",
                Style::parse(&format!("bold {HIGHLIGHT}")).expect("a style"),
            ),
    )
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(why) => {
            eprintln!("top: {why}");
            std::process::exit(2);
        }
    };
    top_app(options, Box::new(Linux::new())).run()
}
