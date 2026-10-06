//! `rich schema` reading SQL DDL and Arrow, comparing any two formats, and
//! drawing ER diagrams (`--er`); `--infer` on `--csv` tables (0.0.16
//! workstream 7). Not upstream.

use std::io::Write;
use std::path::Path;
#[cfg(feature = "arrow")]
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const SHOP: &str = "\
-- The shop.
CREATE TABLE customers (
    id    BIGINT PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    name  TEXT
);
CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers (id),
    total       NUMERIC(10, 2) CHECK (total >= 0)
);
CREATE INDEX orders_customer ON orders (customer_id);
";

fn arrow_fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/schema")
        .join(name)
        .display()
        .to_string()
}

/// A working directory holding the fixtures written as text.
fn dir() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| std::fs::write(temp.path().join(name), text).unwrap();
    write("shop.sql", SHOP);
    write(
        "shop-v2.ddl",
        &SHOP.replace("name  TEXT", "name  TEXT NOT NULL, phone TEXT"),
    );
    write(
        "customers.json",
        r#"{"title": "customers", "required": ["id", "email"],
            "properties": {"id": {"type": "integer"}, "email": {"type": "string"}}}"#,
    );
    write(
        "one.sql",
        "CREATE TABLE customers (id INTEGER NOT NULL, email TEXT NOT NULL, name TEXT);",
    );
    write("broken.sql", "CREATE TABLE t (id INT,\n  name TEXT\n");
    write(
        "orders.csv",
        "id,amount,region,when\n1,4.5,eu,2026-10-01\n2,NA,us,2026-10-02\n3,12,eu,2026-10-03\n",
    );
    temp
}

fn run_in(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "--no-color", "--width", "100"])
        .args(args)
        .current_dir(dir)
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[cfg(feature = "arrow")]
fn path(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

#[test]
fn ddl_draws_as_a_tree_with_the_skipped_clauses_noted() {
    let dir = dir();
    let out = stdout(&run_in(dir.path(), &["schema", "shop.sql"], ""));
    assert_eq!(
        out,
        concat!(
            "shop.sql  2 tables\n",
            "├── customers  table\n",
            "│   ├── id (required)  BIGINT  primary key\n",
            "│   ├── email (required)  VARCHAR(255)  unique\n",
            "│   └── name  TEXT\n",
            "└── orders  table\n",
            "    ├── id (required)  BIGINT  primary key\n",
            "    ├── customer_id (required)  BIGINT  → customers.id\n",
            "    └── total  NUMERIC(10, 2)\n",
            "\n",
            "shop.sql: line 10: orders.total: CHECK constraint skipped\n",
            "shop.sql: line 12: CREATE INDEX statement skipped\n",
        )
    );
}

#[test]
fn ddl_on_stdin_is_recognised_by_its_first_word() {
    let dir = dir();
    let out = stdout(&run_in(
        dir.path(),
        &["schema", "-"],
        "create table t (id int primary key);",
    ));
    assert!(out.starts_with("schema  1 table\n"), "{out}");
    assert!(out.contains("id (required)  int  primary key"), "{out}");
    // Text that is neither SQL nor JSON is still a JSON error.
    let out = run_in(dir.path(), &["schema", "-"], "digraph { a -> b }");
    assert_eq!(out.status.code(), Some(4));
    assert!(stderr(&out).contains("not JSON"), "{}", stderr(&out));
}

/// A pipe named as a file (`/dev/stdin`, `<(…)`): looking for Arrow's magic
/// bytes read its first six bytes, so `CREATE` was gone before the text was
/// read and the rest failed as JSON.
#[cfg(target_os = "linux")]
#[test]
fn a_pipe_named_as_a_file_is_read_whole() {
    let dir = dir();
    let out = stdout(&run_in(dir.path(), &["schema", "/dev/stdin"], SHOP));
    assert!(out.starts_with("stdin  2 tables\n"), "{out}");
    assert!(out.contains("customer_id (required)  BIGINT"), "{out}");
}

/// `--sanitize` covers file text, but `rich schema` reads its files itself
/// and skipped it: a name's escapes reached the terminal.
#[test]
fn sanitize_covers_schema_files() {
    let dir = dir();
    let write = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
    write(
        "evil.sql",
        "CREATE TABLE \"t\x1b[2J\" (\"c\x1b]0;title\x07\" INT REFERENCES \"t\x1b[2J\");\r\n",
    );
    write("evil2.sql", "CREATE TABLE \"t\x1b[2J\" (d INT);\r\n");
    write(
        "evil.json",
        r#"{"title": "s\u001b[2J", "properties": {"k\u001b]0;x\u0007": {"type": "string", "description": "d\u001b[31m"}}}"#,
    );
    write(
        "evil2.json",
        r#"{"title": "s\u001b[2J", "properties": {"k\u001b]0;x\u0007": {"type": "integer"}}}"#,
    );
    for args in [
        &["--sanitize", "schema", "evil.sql"][..],
        &["--sanitize", "schema", "evil.sql", "evil2.sql"],
        &["--sanitize", "schema", "evil.json"],
        &["--sanitize", "schema", "evil.json", "evil2.json"],
        &["--sanitize", "schema", "evil.json", "evil.sql"],
    ] {
        let out = stdout(&run_in(dir.path(), args, ""));
        assert!(
            !out.contains('\x1b') && !out.contains('\x07'),
            "{args:?}: {out:?}"
        );
        assert!(out.contains('␛'), "{args:?}: {out}");
    }
    let out = stdout(&run_in(
        dir.path(),
        &["--sanitize", "schema", "evil.sql"],
        "",
    ));
    assert!(out.contains("t␛[2J  table"), "{out}");
    assert!(out.contains("c␛]0;title␇  INT  → t␛[2J"), "{out}");
    // `rich schema` has no upstream output to keep, so, like `view` and the
    // text diff, it sanitizes by default; `--no-sanitize` lets them through.
    let out = stdout(&run_in(dir.path(), &["schema", "evil.sql"], ""));
    assert!(!out.contains('\x1b') && out.contains("t␛[2J"), "{out:?}");
    let out = stdout(&run_in(
        dir.path(),
        &["schema", "evil.json", "evil2.json"],
        "",
    ));
    assert!(!out.contains('\x1b'), "{out:?}");
    let out = stdout(&run_in(
        dir.path(),
        &["--no-sanitize", "schema", "evil.sql"],
        "",
    ));
    assert!(out.contains("t\x1b[2J"), "{out:?}");
}

#[test]
fn unreadable_ddl_exits_4_with_its_line() {
    let dir = dir();
    let out = run_in(dir.path(), &["schema", "broken.sql"], "");
    assert_eq!(out.status.code(), Some(4));
    assert!(
        stderr(&out).contains("broken.sql: line 1: "),
        "{}",
        stderr(&out)
    );
}

#[test]
fn any_two_formats_diff_through_the_model() {
    let dir = dir();
    let out = stdout(&run_in(
        dir.path(),
        &["schema", "shop.sql", "shop-v2.ddl"],
        "",
    ));
    assert!(
        out.contains("│ ~ │ customers.name  │ became required"),
        "{out}"
    );
    assert!(
        out.contains("│ + │ customers.phone │ field added (TEXT)"),
        "{out}"
    );
    assert!(
        out.contains("shop.sql → shop-v2.ddl: 2 changes, 1 breaking"),
        "{out}"
    );
    // Notes from both files, each named.
    assert!(out.contains("shop.sql: line 12: CREATE INDEX"), "{out}");
    assert!(out.contains("shop-v2.ddl: line 12: CREATE INDEX"), "{out}");

    // A JSON Schema against a file of one table compares with that table.
    let out = stdout(&run_in(
        dir.path(),
        &["schema", "customers.json", "one.sql"],
        "",
    ));
    assert!(out.contains("│ ~ │ email │ type string → TEXT"), "{out}");
    assert!(out.contains("│ + │ name  │ field added (TEXT)"), "{out}");
    assert!(out.contains("customers.json → one.sql: "), "{out}");
}

#[test]
fn er_draws_tables_and_their_keys() {
    let dir = dir();
    let out = stdout(&run_in(
        dir.path(),
        &["schema", "--er", "shop.sql", "--width", "120"],
        "",
    ));
    assert!(out.contains("customers"), "{out}");
    assert!(out.contains("customer_id  BIGINT"), "{out}");
    assert!(out.contains("PK"), "{out}");
    assert!(out.contains("(N:1)"), "{out}");
    assert!(out.contains("shop.sql: line 12: CREATE INDEX"), "{out}");

    // A JSON Schema is one entity.
    let out = stdout(&run_in(
        dir.path(),
        &["schema", "--er", "customers.json"],
        "",
    ));
    assert!(out.contains("customers") && out.contains("email"), "{out}");

    let two = run_in(dir.path(), &["schema", "--er", "shop.sql", "one.sql"], "");
    assert_eq!(two.status.code(), Some(2));
    assert!(
        stderr(&two).contains("--er draws one schema"),
        "{}",
        stderr(&two)
    );
    let elsewhere = run_in(dir.path(), &["--er", "shop.sql"], "");
    assert_eq!(elsewhere.status.code(), Some(2));
    assert!(
        stderr(&elsewhere).contains("--er only has an effect with `rich schema`"),
        "{}",
        stderr(&elsewhere)
    );
}

#[cfg(feature = "arrow")]
#[test]
fn arrow_files_and_streams_draw_and_diff() {
    let dir = dir();
    let v1 = arrow_fixture("events-v1.arrow");
    let v2 = arrow_fixture("events-v2.arrows");
    let out = stdout(&run_in(dir.path(), &["schema", &v1], ""));
    assert!(out.starts_with("events-v1.arrow  8 fields\n"), "{out}");
    assert!(out.contains("├── id (required)  Int32\n"), "{out}");
    assert!(out.contains("Timestamp(µs, \"Europe/Paris\")"), "{out}");

    let out = stdout(&run_in(dir.path(), &["schema", &v1, &v2], ""));
    assert!(
        out.contains("│ ~ │ id              │ type Int32 → Int64"),
        "{out}"
    );
    assert!(
        out.contains("events-v1.arrow → events-v2.arrows: "),
        "{out}"
    );

    // An Arrow file is known by its magic bytes whatever its name.
    let renamed = path(&dir, "events.bin");
    std::fs::copy(&v1, &renamed).unwrap();
    let out = stdout(&run_in(dir.path(), &["schema", "events.bin"], ""));
    assert!(out.starts_with("events.bin  8 fields\n"), "{out}");

    // Not Arrow at all: exit 4.
    std::fs::write(path(&dir, "fake.arrow"), "id,name\n").unwrap();
    let out = run_in(dir.path(), &["schema", "fake.arrow"], "");
    assert_eq!(out.status.code(), Some(4));
    assert!(
        stderr(&out).contains("not an Arrow IPC file or stream"),
        "{}",
        stderr(&out)
    );
}

#[cfg(not(feature = "arrow"))]
#[test]
fn arrow_without_the_feature_is_a_usage_error_naming_it() {
    let dir = dir();
    let out = run_in(
        dir.path(),
        &["schema", &arrow_fixture("events-v1.arrow")],
        "",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("needs rich built with the `arrow` feature"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn infer_types_csv_columns_and_is_opt_in() {
    let dir = dir();
    let plain = stdout(&run_in(dir.path(), &["--csv", "orders.csv"], ""));
    // Without --infer, rich-cli's rule: `NA` makes `amount` text.
    assert!(plain.contains("│ NA     │"), "{plain}");
    assert!(!plain.contains("integer"), "{plain}");

    for args in [
        &["--csv", "orders.csv", "--infer"][..],
        &["orders.csv", "--infer"],
    ] {
        let out = stdout(&run_in(dir.path(), args, ""));
        assert_eq!(
            out,
            concat!(
                "┏━━━━━━━━━┳━━━━━━━━┳━━━━━━━━┳━━━━━━━━━━━━┓\n",
                "┃      id ┃ amount ┃ region ┃ when       ┃\n",
                "┃ integer ┃  float ┃ text   ┃ date       ┃\n",
                "┡━━━━━━━━━╇━━━━━━━━╇━━━━━━━━╇━━━━━━━━━━━━┩\n",
                "│       1 │    4.5 │ eu     │ 2026-10-01 │\n",
                "│       2 │     NA │ us     │ 2026-10-02 │\n",
                "│       3 │     12 │ eu     │ 2026-10-03 │\n",
                "└─────────┴────────┴────────┴────────────┘\n",
            ),
            "{args:?}"
        );
    }
    let out = run_in(dir.path(), &["--json", "-", "--infer"], "{}");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--infer"), "{}", stderr(&out));
}
