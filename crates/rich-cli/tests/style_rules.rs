//! `--style-rule`, `--rules` and a config profile's `[[rules]]` (0.0.19
//! workstream 15, #701): conditional styles for `--csv`, `rich profile` and
//! `rich inspect --table`.

use std::process::{Command, Output, Stdio};

const SERVICES: &str = "service,latency,status\nweb,120,ok\napi,612,failed\ndb,35,ok\n";

/// Run `rich` in a fresh directory holding `files`, 80 columns wide, with no
/// config but the one the files may hold.
fn run_with(files: &[(&str, &str)], args: &[&str]) -> Output {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(home.join(".config/rich")).unwrap();
    for (name, content) in files {
        std::fs::write(work.join(name), content).unwrap();
    }
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "80")
        .env("TERM", "xterm-256color")
        .env_remove("COLORTERM")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn run(args: &[&str]) -> Output {
    run_with(&[("services.csv", SERVICES)], args)
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn line<'a>(out: &'a str, word: &str) -> &'a str {
    out.lines()
        .find(|l| l.contains(word))
        .unwrap_or_else(|| panic!("no line with {word}: {out}"))
}

#[test]
fn a_cell_rule_styles_matching_cells_of_a_csv_table() {
    let out = stdout(&run(&[
        "--csv",
        "services.csv",
        "--force-terminal",
        "--style-rule",
        "latency>500:bold red",
    ]));
    // 612 is over 500; the column's own bold green is underneath.
    assert!(line(&out, "api").contains("\x1b[1;31m612"), "{out:?}");
    assert!(!line(&out, "web").contains("31m"), "{out:?}");
    // A rule only restyles: the plain table is the same table.
    let plain = stdout(&run(&[
        "--csv",
        "services.csv",
        "--style-rule",
        "latency>500:red",
    ]));
    assert_eq!(plain, stdout(&run(&["--csv", "services.csv"])));
}

#[test]
fn row_and_column_targets_and_rule_files() {
    let out = stdout(&run(&[
        "services.csv",
        "--force-terminal",
        "--style-rule",
        "status=failed:row:on red",
    ]));
    let failed = line(&out, "api");
    assert!(failed.contains("41m"), "{failed:?}");
    assert!(!line(&out, "web").contains("41m"), "{out:?}");
    // The same rules from a file, in TOML rule tables; a column rule styles
    // every cell of a column when any cell matches.
    let rules =
        "[[rules]]\ncolumn = 'status'\nvalue = 'failed'\nstyle = 'on red'\ntarget = 'row'\n\
                 [[rules]]\ncolumn = 'service'\nop = 'starts_with'\nvalue = 'd'\nstyle = 'italic'\n\
                 target = 'column'\n";
    let out = stdout(&run_with(
        &[("services.csv", SERVICES), ("rules.toml", rules)],
        &["services.csv", "--force-terminal", "--rules", "rules.toml"],
    ));
    assert!(line(&out, "api").contains("41m"), "{out:?}");
    assert!(line(&out, "web").contains("\x1b[3mweb"), "{out:?}");
    // `#N` names a column by index, and works without a header.
    let out = stdout(&run_with(
        &[("n.csv", "1,2\n3,4\n")],
        &[
            "--csv",
            "n.csv",
            "--force-terminal",
            "--style-rule",
            "#1>=4:underline",
        ],
    ));
    assert!(out.contains("\x1b[1;4;32m4"), "{out:?}");
    assert!(!out.contains("4;32m2"), "{out:?}");
}

#[test]
fn markup_in_cells_survives_a_rule() {
    // Upstream renders CSV cells as markup; a styled cell still does.
    let csv = "name,score\n[b]bold[/b],5\nplain,1\n";
    let out = stdout(&run_with(
        &[("m.csv", csv)],
        &[
            "--csv",
            "m.csv",
            "--force-terminal",
            "--style-rule",
            "score>3:row:italic",
        ],
    ));
    let styled = line(&out, "bold");
    assert!(!styled.contains("[b]"), "{styled:?}");
    assert!(styled.contains("3m"), "{styled:?}");
}

#[test]
fn bad_rules_are_usage_errors_naming_the_rule() {
    let out = run(&["--csv", "services.csv", "--style-rule", "latency>:red"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--style-rule: rule \"latency>:red\": no value after `>`"),
        "{stderr}"
    );
    let out = run(&[
        "--csv",
        "services.csv",
        "--style-rule",
        "latency>5:nonsense colour",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("style \"nonsense colour\""));
    let out = run_with(
        &[
            ("services.csv", SERVICES),
            (
                "bad.toml",
                "[[rules]]\ncolumn = 'a'\nop = 'like'\nvalue = 1\nstyle = 'red'\n",
            ),
        ],
        &["--csv", "services.csv", "--rules", "bad.toml"],
    );
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--rules bad.toml: rules[0]: unknown op `like`"),
        "{stderr}"
    );
    let out = run(&["--csv", "services.csv", "--rules", "missing.toml"]);
    assert_eq!(out.status.code(), Some(2));
    // A rule for a column the table lacks is drawn without, with a warning.
    let out = run(&["--csv", "services.csv", "--style-rule", "latncy>5:red"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains(
            "warning: style rule \"latncy>5:red\" names no column of this table (columns: \
             service, latency, status)"
        ),
        "{stderr}"
    );
}

#[test]
fn rules_have_no_effect_where_no_data_table_is_drawn() {
    // Accepted, as a config profile's rules must be with any command.
    let out = run_with(&[("a.md", "# hi\n")], &["a.md", "--style-rule", "x>1:red"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_config_profile_carries_rules_and_flags_add_to_them() {
    let config = "[defaults]\n\
                  [[defaults.rules]]\ncolumn = 'latency'\nop = '>'\nvalue = 500\nstyle = 'bold red'\n\
                  [profiles.tables]\n\
                  [[profiles.tables.rules]]\ncolumn = 'status'\nvalue = 'failed'\nstyle = 'on red'\n\
                  target = 'row'\n";
    let files = [("services.csv", SERVICES), ("rich.toml", config)];
    // The defaults' rule.
    let out = stdout(&run_with(&files, &["services.csv", "--force-terminal"]));
    assert!(line(&out, "api").contains("\x1b[1;31m612"), "{out:?}");
    assert!(!line(&out, "api").contains("41m"), "{out:?}");
    // The profile's rules add to the defaults', and a flag adds to both.
    let out = stdout(&run_with(
        &files,
        &[
            "services.csv",
            "--force-terminal",
            "--profile",
            "tables",
            "--style-rule",
            "service=db:italic",
        ],
    ));
    assert!(line(&out, "api").contains("\x1b[1;31;41m612"), "{out:?}");
    assert!(line(&out, "db").contains("\x1b[3mdb"), "{out:?}");
    // Rules never stop another command.
    let out = run_with(
        &[("rich.toml", config), ("a.md", "# hi\n")],
        &["a.md", "--profile", "tables"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // `config show` lists them; a bad rule table fails validation, named.
    let out = stdout(&run_with(
        &files,
        &["config", "show", "--profile", "tables"],
    ));
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["settings"]["rules"].as_array().unwrap().len(), 2);
    let bad = "[profiles.ci]\n[[profiles.ci.rules]]\ncolumn = 'a'\nstyle = 'red'\n";
    let out = run_with(&[("rich.toml", bad)], &["config", "validate"]);
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("profiles.ci.rules[0]: missing `value`"),
        "{stderr}"
    );
}

#[test]
fn inspect_table_records_are_styled_by_rules() {
    let json = r#"[{"name": "web", "p99": 120}, {"name": "api", "p99": 612}, {"name": "db"}]"#;
    let out = stdout(&run_with(
        &[("s.json", json)],
        &[
            "inspect",
            "s.json",
            "--table",
            "--force-terminal",
            "--style-rule",
            "p99>500:bold red",
            "--style-rule",
            "p99 empty:row:dim",
        ],
    ));
    assert!(line(&out, "api").contains("\x1b[1;31m612"), "{out:?}");
    assert!(line(&out, "db").contains("\x1b[2m"), "{out:?}");
    assert!(!line(&out, "web").contains("\x1b[2m"), "{out:?}");
}
