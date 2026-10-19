//! The bottom (btm) rebuild (examples/top.rs): its units, its search and
//! process table, and the app driven by keys on scripted samples.

#[path = "../examples/top.rs"]
#[allow(dead_code)]
mod top;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use intuituive::widgets::Order;
use intuituive::{App, Driver};
use top::*;

fn proc(pid: u32, ppid: u32, name: &str, cpu: f64, mem_mib: u64, user: &str) -> Proc {
    Proc {
        pid,
        ppid,
        name: name.into(),
        command: format!("/usr/bin/{name} --serve"),
        cpu,
        mem: mem_mib * 1024 * 1024,
        mem_percent: mem_mib as f64 / 16384.0 * 100.0,
        read: 2_000,
        write: 0,
        total_read: 3_500_000,
        total_write: 1_200,
        user: user.into(),
        state: "Sleeping".into(),
        time: Duration::from_secs(75),
        kernel: false,
    }
}

fn procs() -> Vec<Proc> {
    vec![
        proc(1, 0, "systemd", 0.1, 12, "root"),
        proc(400, 1, "sshd", 0.0, 8, "root"),
        proc(410, 400, "bash", 0.3, 6, "dev"),
        proc(420, 410, "cargo", 41.5, 900, "dev"),
        proc(421, 420, "rustc", 23.0, 1500, "dev"),
        proc(500, 1, "postgres", 6.4, 412, "postgres"),
        proc(501, 500, "postgres", 1.2, 64, "postgres"),
        proc(600, 1, "nginx", 0.8, 36, "www-data"),
    ]
}

/// The `n`th sample: the cores and the network ramp up, the rest is still.
fn sample(n: u64) -> Sample {
    let n = n as f64;
    Sample {
        cpus: vec![10.0 + n, 50.0, 90.0 - n, 5.0],
        load: [0.52, 0.48, 0.40],
        ram: Mem {
            used: 4 * 1024 * 1024 * 1024,
            total: 16 * 1024 * 1024 * 1024,
        },
        swap: Mem {
            used: 512 * 1024 * 1024,
            total: 2 * 1024 * 1024 * 1024,
        },
        net: Net {
            rx: (1_000.0 + 100.0 * n) as u64,
            tx: 300,
            total_rx: 8 * 3_400_000_000,
            total_tx: 8 * 120_000_000,
        },
        temps: vec![
            Temp {
                name: "k10temp: Tctl".into(),
                celsius: 54.0,
            },
            Temp {
                name: "nvme: Composite".into(),
                celsius: 38.5,
            },
            Temp {
                name: "amdgpu: edge".into(),
                celsius: 61.0,
            },
        ],
        disks: vec![
            Disk {
                name: "/dev/nvme0n1p2".into(),
                mount: "/".into(),
                used: 120 * 1024 * 1024 * 1024,
                free: 380 * 1024 * 1024 * 1024,
                total: 500 * 1024 * 1024 * 1024,
                read: 0,
                write: 4_096,
            },
            Disk {
                name: "/dev/nvme0n1p1".into(),
                mount: "/boot/efi".into(),
                used: 60 * 1024 * 1024,
                free: 450 * 1024 * 1024,
                total: 510 * 1024 * 1024,
                read: 0,
                write: 0,
            },
        ],
        procs: procs(),
    }
}

/// The signals the app sent: `(pid, signal)`.
type Kills = Arc<Mutex<Vec<(u32, i32)>>>;

/// Scripted samples, and the signals the app sent.
struct Fake {
    n: u64,
    kills: Kills,
}

impl Source for Fake {
    fn sample(&mut self) -> Sample {
        self.n += 1;
        sample(self.n)
    }

    fn kill(&mut self, pid: u32, signal: i32) -> Result<(), String> {
        if pid == 1 {
            return Err("Operation not permitted".into());
        }
        self.kills.lock().unwrap().push((pid, signal));
        Ok(())
    }
}

fn app_with(options: Options) -> (App, Kills) {
    let kills = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        n: 0,
        kills: kills.clone(),
    };
    (top_app(options, Box::new(fake)).wait_for_tasks(true), kills)
}

/// The app at `width` x `height`, a few samples in.
fn driver_with(options: Options, width: u16, height: u16) -> (Driver, Kills) {
    let (app, kills) = app_with(options);
    let mut driver = app.driver(width, height);
    driver.advance(Duration::from_millis(3_100));
    (driver, kills)
}

fn driver() -> Driver {
    driver_with(Options::default(), 120, 40).0
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn has(driver: &Driver, needle: &str) -> bool {
    rows(driver).iter().any(|r| r.contains(needle))
}

/// The process table's rows: from its header to the panel's bottom edge,
/// in the bottom right quarter at 120 x 40.
fn processes(driver: &Driver) -> Vec<String> {
    let rows = rows(driver);
    let header = (rows.iter())
        .position(|r| r.contains("│PID ") || r.contains("│Count "))
        .expect("the process header");
    rows[header + 1..]
        .iter()
        .map(|r| {
            let cell: String = r.chars().skip(61).collect();
            cell.trim_start_matches('│')
                .trim_end_matches('│')
                .trim_end()
                .to_string()
        })
        .take_while(|r| !r.is_empty() && !r.starts_with('└'))
        .collect()
}

fn names(driver: &Driver) -> Vec<String> {
    processes(driver)
        .iter()
        .map(|r| r.split_whitespace().nth(1).unwrap_or("").to_string())
        .collect()
}

fn braille(driver: &Driver) -> usize {
    rows(driver)
        .iter()
        .flat_map(|r| r.chars())
        .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
        .count()
}

#[test]
fn units_are_bottoms() {
    assert_eq!(binary_string(900 * 1024 * 1024), "900MiB");
    assert_eq!(binary_string(3 * 1024 * 1024 * 1024 / 2), "1.5GiB");
    assert_eq!(decimal_string(3_500_000), "4MB");
    assert_eq!(decimal_string(3_400_000_000), "3.4GB");
    assert_eq!(per_second(2_000), "2KB/s");
    assert_eq!(per_second(0), "0B/s");
    assert_eq!(format_time(Duration::from_millis(9_250)), "9.250s");
    assert_eq!(format_time(Duration::from_secs(75)), "1m 15.00s");
    assert_eq!(
        format_time(Duration::from_secs(3 * 3600 + 120 + 5)),
        "3h 2m 5s"
    );
    assert_eq!(
        format_time(Duration::from_secs(2 * 86_400 + 3600)),
        "2d 1h 0m"
    );
    let ram = Mem {
        used: 4 * 1024 * 1024 * 1024,
        total: 16 * 1024 * 1024 * 1024,
    };
    assert_eq!(memory_label("RAM", &ram), "RAM: 25%   4.0GiB/16.0GiB");
    assert_eq!(
        memory_label("SWP", &Mem::default()),
        "SWP:   0%   0.0B/0.0B"
    );
    assert_eq!(bits_label(1_400), "1.4Kb/s");
    assert_eq!(bits_label(300), "300.0b/s");
    // The network's top is half again the largest rate, its labels in the
    // unit of that top.
    let (top, labels) = network_axis(1_400.0);
    assert_eq!(top, 2_100.0);
    assert_eq!(labels, ["  0Kb", "  0.7", "  1.4", "  2.1"]);
    assert_eq!(network_axis(0.0).0, 1.0);
}

#[test]
fn the_history_slides_and_keeps_ten_minutes() {
    let mut h = History::default();
    for n in 0..700 {
        h.push(&sample(n), 1000);
    }
    // Ten minutes and a sample, at one a second.
    assert_eq!(h.points.len(), 602);
    let window = h.series(60_000, |p| p.avg);
    // From just before -60 s (so the line reaches the edge) to 0.
    assert_eq!(window.first().unwrap().0, -61_000.0);
    assert_eq!(window.last().unwrap().0, 0.0);
    assert_eq!(window.len(), 62);
    assert!(History::default().series(60_000, |p| p.ram).is_empty());
}

#[test]
fn the_search_reads_bottoms_queries() {
    let all = procs();
    let found = |text: &str, case: bool, whole: bool| -> Vec<u32> {
        let q = Query {
            text: text.into(),
            case,
            whole,
            regex: false,
        };
        all.iter()
            .filter(|p| q.matches(p, false))
            .map(|p| p.pid)
            .collect()
    };
    assert_eq!(found("post", false, false), [500, 501]);
    assert_eq!(found("POST", false, false), [500, 501]);
    assert!(found("POST", true, false).is_empty(), "matching case");
    assert!(found("post", false, true).is_empty(), "the whole word");
    assert_eq!(found("cpu > 20", false, false), [420, 421]);
    assert_eq!(found("cpu>20", false, false), [420, 421]);
    assert_eq!(found("user = root and !sshd", false, false), [1]);
    assert_eq!(found("nginx or pid = 1", false, false), [1, 600]);
    assert_eq!(found("memb > 1 gib", false, false), [421]);
    assert_eq!(found("", false, false).len(), all.len());
    let bad = Query {
        text: "cpu > lots".into(),
        ..Query::default()
    };
    assert_eq!(bad.error().as_deref(), Some("Invalid number: lots"));
    assert!(
        all.iter().all(|p| bad.matches(p, false)),
        "a bad query hides nothing"
    );
}

#[test]
fn rows_sort_group_and_make_a_tree() {
    let all = procs();
    let none = BTreeSet::new();
    let view = ProcView::default();
    let pids = |rows: &[ProcRow]| rows.iter().map(|r| r.pids.clone()).collect::<Vec<_>>();
    // CPU, largest first, by default.
    let rows = proc_rows(&all, &view, &Query::default(), &none);
    assert_eq!(rows[0].pids, [420]);
    assert_eq!(rows[0].cells[..4], ["420", "cargo", "41.5%", "5.5%"]);
    // Grouped by name, the counts first and the usage summed.
    let grouped = ProcView {
        grouped: true,
        sort: Some((1, Order::Ascending)),
        ..view
    };
    let rows = proc_rows(&all, &grouped, &Query::default(), &none);
    let postgres = rows.iter().find(|r| r.name == "postgres").unwrap();
    assert_eq!(postgres.pids, [500, 501]);
    assert_eq!(postgres.cells[..3], ["2", "postgres", "7.6%"]);
    assert_eq!(grouped.titles()[0], "Count");
    // A tree: children under their parents, each level sorted.
    let tree = ProcView { tree: true, ..view };
    let rows = proc_rows(&all, &tree, &Query::default(), &none);
    let shown: Vec<&str> = rows.iter().map(|r| r.cells[1].as_str()).collect();
    assert_eq!(
        shown,
        [
            "systemd",
            "├─ postgres",
            "│  └─ postgres",
            "├─ nginx",
            "└─ sshd",
            "   └─ bash",
            "      └─ cargo",
            "         └─ rustc",
        ]
    );
    // Collapsed, a parent shows `+` and hides its descendants.
    let shut = BTreeSet::from([410]);
    let rows = proc_rows(&all, &tree, &Query::default(), &shut);
    assert_eq!(rows.last().unwrap().cells[1], "   └─ + bash");
    // A search keeps the ancestors of what it finds.
    let q = Query {
        text: "rustc".into(),
        ..Query::default()
    };
    assert_eq!(
        pids(&proc_rows(&all, &tree, &q, &none)),
        [vec![1], vec![400], vec![410], vec![420], vec![421]]
    );
}

#[test]
fn the_default_layout_is_bottoms() {
    let d = driver();
    let rows = rows(&d);
    // CPU over the whole width, its legend at the right.
    assert!(rows[0].starts_with("┌─ CPU ─"), "{rows:#?}");
    assert!(rows[1].contains("│CPU  Use"), "{rows:#?}");
    assert!(rows[3].contains("│AVG  39%"), "{rows:#?}");
    assert!(rows[6].contains("│CPU2 86%"), "{rows:#?}");
    assert!(has(&d, "100%") && has(&d, "  0%"));
    assert!(has(&d, "60s") && has(&d, "0s│"));
    // Memory beside temperatures over disks, network beside processes.
    let row = |needle: &str| rows.iter().position(|r| r.contains(needle)).unwrap();
    assert_eq!(row("─ Memory ─"), row("─ Temperatures ─"));
    assert!(row("─ Disks ─") > row("─ Temperatures ─"));
    assert_eq!(row("─ Network ─"), row("─ Processes ─"));
    assert!(has(&d, "RAM: 25%   4.0GiB/16.0GiB"));
    assert!(has(&d, "SWP: 25%   0.5GiB/2.0GiB"));
    assert!(has(&d, "RX: 1.4Kb/s    All: 3.4GB"));
    assert!(has(&d, "k10temp: Tctl") && has(&d, "54°C"));
    assert!(has(&d, "/dev/nvme0n1p2 /         120.0GiB 380.0GiB"));
    // The graphs have drawn the samples so far.
    assert!(braille(&d) > 20);
    // bottom starts on the processes, sorted by CPU.
    d.assert_focused("processes");
    assert!(has(&d, "CPU% ▼"));
    assert_eq!(names(&d)[..3], ["cargo", "rustc", "postgres"]);
}

#[test]
fn the_arrows_move_between_widgets_as_laid_out() {
    let mut d = driver();
    d.press("K");
    d.assert_focused("disks");
    d.press("shift+up");
    d.assert_focused("temperatures");
    d.press("H");
    d.assert_focused("memory");
    d.press("ctrl+up");
    d.assert_focused("cpu");
    d.press("L");
    d.assert_focused("cpu legend");
    d.press("J");
    d.assert_focused("temperatures");
    d.press("S S");
    d.assert_focused("processes");
    d.press("A");
    d.assert_focused("network");
    d.press("D");
    d.assert_focused("processes");
    // Nothing further right: the focus stays.
    d.press("ctrl+right");
    d.assert_focused("processes");
}

#[test]
fn e_expands_the_selected_widget_and_esc_goes_back() {
    let mut d = driver();
    d.press("e");
    let screen = rows(&d);
    assert!(screen[0].starts_with("┌─ Processes ─"), "{screen:#?}");
    assert!(screen[0].contains("Esc to go back"), "{screen:#?}");
    assert!(!has(&d, "─ CPU ─") && !has(&d, "─ Memory ─"));
    // Every column fits now.
    assert!(
        screen[1].contains("T.Write User     State    Time"),
        "{screen:#?}"
    );
    // The widget keeps its keys, and the arrows do nothing.
    d.press("n");
    assert!(has(&d, "Name ▲"));
    d.press("K");
    d.assert_focused("processes");
    d.press("esc");
    assert!(has(&d, "─ CPU ─") && has(&d, "─ Memory ─"));
    d.assert_focused("processes");
    // A graph expands too, and `e` brings it back.
    d.press("H K e");
    assert!(rows(&d)[0].starts_with("┌─ Memory ─"));
    d.assert_focused("memory");
    d.press("e");
    d.assert_focused("memory");
    assert!(has(&d, "─ Processes ─"));
}

#[test]
fn plus_and_minus_zoom_the_selected_graph() {
    let mut d = driver();
    d.press("H K");
    d.assert_focused("memory");
    d.press("minus");
    assert!(has(&d, "│  75s"), "{:#?}", rows(&d));
    // The others keep their window.
    assert!(has(&d, "│  60s"));
    d.press("plus plus plus");
    assert!(has(&d, "│  30s"), "{:#?}", rows(&d));
    // Not below 30 s.
    d.press("plus");
    assert!(has(&d, "│  30s"));
    d.press("equals");
    assert!(!has(&d, "│  30s"));
}

#[test]
fn the_legend_picks_what_the_cpu_graph_shows() {
    let mut d = driver();
    d.press("H K K L");
    d.assert_focused("cpu legend");
    let all = braille(&d);
    // AVG, then one core: fewer lines on the graph.
    d.press("j");
    let avg = braille(&d);
    assert!(avg < all, "{avg} < {all}");
    d.press("j j");
    assert!(braille(&d) < all);
    d.press("g");
    assert_eq!(braille(&d), all);
}

#[test]
fn keys_sort_the_processes_and_again_reverse() {
    let mut d = driver();
    d.press("c");
    assert!(has(&d, "CPU% ▲"));
    assert_eq!(names(&d)[0], "sshd");
    d.press("p");
    assert!(has(&d, "PID ▲"));
    assert_eq!(processes(&d)[0].split_whitespace().next(), Some("1"));
    d.press("I");
    assert!(has(&d, "PID ▼"));
    assert_eq!(processes(&d)[0].split_whitespace().next(), Some("600"));
    d.press("n");
    assert_eq!(names(&d)[..3], ["bash", "cargo", "nginx"]);
    d.press("m");
    assert!(has(&d, "Mem% ▼"));
    assert_eq!(names(&d)[0], "rustc");
    // `%` shows the memory as sizes, `P` the whole command.
    d.press("percent");
    assert!(has(&d, "Mem ▼"));
    assert!(processes(&d)[0].contains("1.5GiB"), "{:#?}", processes(&d));
    assert!(processes(&d)[1].contains("900MiB"), "{:#?}", processes(&d));
    d.press("P");
    assert!(has(&d, "│PID Command"));
    assert!(
        processes(&d)[0].contains("/usr/bin/rustc"),
        "{:#?}",
        processes(&d)
    );
}

#[test]
fn the_sort_menu_lists_the_columns() {
    let mut d = driver();
    d.press("s");
    assert!(has(&d, "┌─ Sort By ─┐"), "{:#?}", rows(&d));
    assert!(has(&d, "│T.Write    │"));
    d.press("k k enter");
    assert!(!has(&d, "Sort By"));
    assert!(has(&d, "PID ▲"));
    d.assert_focused("processes");
    d.press("s esc");
    assert!(!has(&d, "Sort By"));
    d.assert_focused("processes");
}

#[test]
fn the_search_filters_as_it_is_typed_and_keeps_its_filter() {
    let mut d = driver();
    d.press("slash");
    assert!(has(&d, "Esc to close"));
    assert!(has(&d, "Case(Alt+C)  Whole(Alt+W)  Regex(Alt+R)"));
    d.assert_focused("search");
    d.type_text("POST");
    assert_eq!(names(&d), ["postgres", "postgres"]);
    // The options: matching case finds none.
    d.press("alt+c");
    assert!(processes(&d).is_empty());
    d.press("f1");
    assert_eq!(names(&d).len(), 2);
    d.press("ctrl+u");
    d.type_text("cpu > lots");
    assert!(has(&d, "Invalid number: lots"));
    d.press("ctrl+u");
    d.type_text("cpu > 20");
    assert_eq!(names(&d), ["cargo", "rustc"]);
    // Esc closes the box and keeps the filter.
    d.press("esc");
    assert!(!has(&d, "Esc to close"));
    d.assert_focused("processes");
    assert_eq!(names(&d), ["cargo", "rustc"]);
    d.press("ctrl+f");
    d.assert_focused("search");
}

#[test]
fn tree_mode_collapses_and_expands_branches() {
    let mut d = driver();
    d.press("t");
    let rows = processes(&d);
    assert!(rows[1].starts_with("500 ├─ postgres"), "{rows:#?}");
    assert!(rows[2].starts_with("501 │  └─ postgres"), "{rows:#?}");
    d.press("j space");
    assert!(processes(&d)[1].starts_with("500 ├─ + postgres"));
    assert_eq!(processes(&d).len(), 7);
    d.press("right");
    assert_eq!(processes(&d).len(), 8);
    d.press("left");
    assert_eq!(processes(&d).len(), 7);
    d.press("minus");
    assert_eq!(processes(&d).len(), 8);
    // Grouping is off in tree mode.
    d.press("tab");
    assert!(has(&d, "│PID Name"));
    d.press("f5");
    assert!(processes(&d)[0].starts_with("420 cargo"));
}

#[test]
fn tab_groups_processes_by_name() {
    let mut d = driver();
    d.press("tab n");
    assert!(has(&d, "│Count Name"), "{:#?}", rows(&d));
    let postgres = (processes(&d).into_iter())
        .find(|r| r.contains("postgres"))
        .unwrap();
    assert!(postgres.starts_with("2     postgres"), "{postgres:?}");
    assert!(postgres.contains("7.6%"), "{postgres:?}");
    d.press("tab");
    assert!(has(&d, "│PID Name"));
}

#[test]
fn dd_asks_which_signal_and_sends_it() {
    let (mut d, kills) = driver_with(Options::default(), 120, 40);
    d.press("d d");
    assert!(has(&d, "Select Signal"), "{:#?}", rows(&d));
    d.assert_focused("signals");
    assert!(has(
        &d,
        "Kill process 'cargo' with PID 420? Press ENTER to confirm."
    ));
    assert!(has(&d, "15: TERM") && has(&d, "0: Cancel"));
    // TERM is picked; one down is STKFLT.
    d.press("enter");
    assert!(!has(&d, "Select Signal"));
    assert_eq!(*kills.lock().unwrap(), [(420, 15)]);
    d.press("j f9 down down enter");
    assert_eq!(kills.lock().unwrap()[1], (421, 17));
    // Esc, or Cancel, sends nothing.
    d.press("delete esc");
    d.press("delete home enter");
    assert_eq!(kills.lock().unwrap().len(), 2);
    // A group's every process.
    d.press("tab n home j j j");
    d.press("d d");
    assert!(
        has(&d, "Kill 2 processes with the name 'postgres'?"),
        "{:#?}",
        rows(&d)
    );
    d.press("enter");
    assert_eq!(kills.lock().unwrap()[2..], [(500, 15), (501, 15)]);
}

#[test]
fn a_failed_kill_says_why() {
    let (mut d, kills) = driver_with(Options::default(), 120, 40);
    d.press("p");
    assert_eq!(processes(&d)[0].split_whitespace().next(), Some("1"));
    d.press("d d enter");
    assert!(
        has(&d, "Failed to kill process systemd (1):"),
        "{:#?}",
        rows(&d)
    );
    assert!(has(&d, "Operation not permitted"));
    assert!(kills.lock().unwrap().is_empty());
}

#[test]
fn without_advanced_kill_it_is_yes_or_no() {
    let options = Options {
        advanced_kill: false,
        ..Options::default()
    };
    let (mut d, kills) = driver_with(options, 120, 40);
    d.press("d d");
    assert!(has(&d, "Confirm Kill Process"), "{:#?}", rows(&d));
    assert!(has(&d, "[Y]es"));
    d.press("n");
    assert!(kills.lock().unwrap().is_empty());
    d.press("f9 y");
    assert_eq!(*kills.lock().unwrap(), [(420, 15)]);
}

#[test]
fn ctrl_d_and_ctrl_u_move_half_a_page() {
    let mut d = driver();
    // The table shows 9 rows of processes: half is 4.
    d.press("ctrl+d d d");
    assert!(has(&d, "PID 600?"), "{:#?}", rows(&d));
    d.press("esc ctrl+u d d");
    assert!(has(&d, "PID 420?"), "{:#?}", rows(&d));
}

#[test]
fn the_help_lists_bottoms_sections() {
    let mut d = driver();
    d.press("question");
    assert!(has(&d, "─ Help ─") && has(&d, "Esc to close"));
    assert!(has(&d, "1 - General"));
    d.assert_focused("help");
    assert!(has(
        &d,
        "e                       Toggle expanding the currently selected widget"
    ));
    d.press("pagedown");
    assert!(
        has(&d, "t, F5                   Toggle tree mode"),
        "{:#?}",
        rows(&d)
    );
    d.press("esc");
    assert!(!has(&d, "─ Help ─"));
    d.assert_focused("processes");
}

#[test]
fn f_freezes_the_display() {
    let mut d = driver();
    let legend = |d: &Driver| {
        rows(d)
            .iter()
            .find(|r| r.contains("│CPU0 "))
            .map(|r| r.chars().rev().take(18).collect::<String>())
            .unwrap()
    };
    let before = legend(&d);
    d.press("f");
    assert!(has(&d, "Frozen, press 'f' to unfreeze"));
    d.advance(Duration::from_millis(2_000));
    assert_eq!(legend(&d), before);
    d.press("f");
    assert!(!has(&d, "Frozen"));
    d.advance(Duration::from_millis(1_000));
    assert_ne!(legend(&d), before);
}

#[test]
fn ctrl_r_resets_the_graphs() {
    let mut d = driver();
    let drawn = braille(&d);
    d.press("ctrl+r");
    assert!(braille(&d) < drawn);
}

#[test]
fn basic_mode_has_gauges_and_one_table_at_a_time() {
    let options = Options {
        basic: true,
        ..Options::default()
    };
    let (mut d, _) = driver_with(options, 120, 30);
    let screen = rows(&d);
    assert!(screen[0].starts_with(" AVG [|||"), "{screen:#?}");
    assert!(screen[0].contains("39%]"), "{screen:#?}");
    assert!(has(&d, "RAM [|||") && has(&d, "4.0GiB/16.0GiB]"));
    assert!(has(&d, "RX: 1.4Kb/s"));
    assert!(!has(&d, "─ CPU ─") && !has(&d, "─ Memory ─"));
    assert!(has(&d, "─ Processes ─") && has(&d, "Temperatures ►"));
    d.press("L");
    assert!(has(&d, "─ Temperatures ─") && has(&d, "◄ Processes") && has(&d, "Disks ►"));
    d.press("ctrl+right");
    assert!(has(&d, "─ Disks ─") && has(&d, "◄ Temperatures"));
    d.press("H H");
    assert!(has(&d, "─ Processes ─"));
    // No expanding.
    d.press("e");
    assert!(has(&d, "AVG [|||"));
}

#[test]
fn the_options_are_bottoms() {
    let args = |text: &str| {
        text.split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    assert_eq!(parse(&[]).unwrap(), Options::default());
    let options = parse(&args(
        "-b -r 500 --default_time_value 120000 --disable_advanced_kill",
    ))
    .unwrap();
    assert!(options.basic && !options.advanced_kill);
    assert_eq!((options.rate, options.default_time), (500, 120_000));
    assert!(parse(&args("--rate")).is_err());
    assert!(parse(&args("--bogus")).is_err());
}

#[test]
fn a_small_terminal_still_draws_every_widget() {
    let (d, _) = driver_with(Options::default(), 80, 24);
    for title in [
        "CPU",
        "Memory",
        "Temperatures",
        "Disks",
        "Network",
        "Processes",
    ] {
        assert!(has(&d, &format!("─ {title} ─")), "{title}: {:#?}", rows(&d));
    }
}

#[test]
fn the_wheel_zooms_a_graph() {
    use intuituive::interact::{Event, Mouse, MouseKind};
    let mut d = driver();
    // Over the memory graph, at the left of the second band.
    let wheel = |kind| Event::Mouse(Mouse::new(kind, 30, 20));
    d.event(wheel(MouseKind::ScrollDown));
    d.settle();
    assert!(has(&d, "│  75s"), "{:#?}", rows(&d));
    d.event(wheel(MouseKind::ScrollUp));
    d.event(wheel(MouseKind::ScrollUp));
    d.settle();
    assert!(has(&d, "│  45s"), "{:#?}", rows(&d));
}
