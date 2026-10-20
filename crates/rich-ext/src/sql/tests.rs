use super::*;
use TokenKind::*;

/// The non-whitespace tokens of `code`, with their text.
fn tokens(code: &str) -> Vec<(TokenKind, &str)> {
    tokenize(code)
        .into_iter()
        .filter(|t| t.kind != Whitespace)
        .map(|t| (t.kind, &code[t.range]))
        .collect()
}

#[test]
fn tokens_cover_the_input_in_order() {
    let code = "SELECT a.b, count(*) FROM \"T\" WHERE x <> 'y''z' -- c\n/* d */;";
    let all = tokenize(code);
    let mut at = 0;
    for token in &all {
        assert_eq!(token.range.start, at);
        assert!(token.range.end > token.range.start);
        at = token.range.end;
    }
    assert_eq!(at, code.len());
}

#[test]
fn keywords_names_functions_and_types() {
    assert_eq!(
        tokens("select Count(id), t.select, cast(x AS VARCHAR(10)) from tbl"),
        [
            (Keyword, "select"),
            (Function, "Count"),
            (Punctuation, "("),
            (Name, "id"),
            (Punctuation, ")"),
            (Punctuation, ","),
            (Name, "t"),
            (Punctuation, "."),
            (Name, "select"),
            (Punctuation, ","),
            (Keyword, "cast"),
            (Punctuation, "("),
            (Name, "x"),
            (Keyword, "AS"),
            (Type, "VARCHAR"),
            (Punctuation, "("),
            (Number, "10"),
            (Punctuation, ")"),
            (Punctuation, ")"),
            (Keyword, "from"),
            (Name, "tbl"),
        ]
    );
    assert_eq!(
        tokens("QUALIFY PRAGMA null TRUE int64 timestamptz"),
        [
            (Keyword, "QUALIFY"),
            (Keyword, "PRAGMA"),
            (Constant, "null"),
            (Constant, "TRUE"),
            (Type, "int64"),
            (Type, "timestamptz"),
        ]
    );
}

#[test]
fn quoted_names() {
    assert_eq!(
        tokens(r#""a ""b""" `c``d` [e]]f] [dbo].[t]"#),
        [
            (QuotedName, r#""a ""b""""#),
            (QuotedName, "`c``d`"),
            (QuotedName, "[e]]f]"),
            (QuotedName, "[dbo]"),
            (Punctuation, "."),
            (QuotedName, "[t]"),
        ]
    );
    // Subscripts are not names.
    assert_eq!(
        tokens("arr[1] ARRAY[2] f(x)[3]"),
        [
            (Name, "arr"),
            (Punctuation, "["),
            (Number, "1"),
            (Punctuation, "]"),
            (Keyword, "ARRAY"),
            (Punctuation, "["),
            (Number, "2"),
            (Punctuation, "]"),
            (Function, "f"),
            (Punctuation, "("),
            (Name, "x"),
            (Punctuation, ")"),
            (Punctuation, "["),
            (Number, "3"),
            (Punctuation, "]"),
        ]
    );
}

#[test]
fn strings() {
    assert_eq!(
        tokens(r"'it''s' '''' E'a\'b' N'x' X'FF' r'\d' $$ a ' b $$ $fn$ x $fn$"),
        [
            (String, "'it''s'"),
            (String, "''''"),
            (String, r"E'a\'b'"),
            (String, "N'x'"),
            (String, "X'FF'"),
            (String, r"r'\d'"),
            (String, "$$ a ' b $$"),
            (String, "$fn$ x $fn$"),
        ]
    );
    assert_eq!(
        tokens("'''it's''' \"\"\"q\"\"\" ''"),
        [
            (String, "'''it's'''"),
            (String, "\"\"\"q\"\"\""),
            (String, "''"),
        ]
    );
}

#[test]
fn numbers() {
    assert_eq!(
        tokens("1 1.5 .5 1e10 2.5E-3 0xFF 1_000 t.5"),
        [
            (Number, "1"),
            (Number, "1.5"),
            (Number, ".5"),
            (Number, "1e10"),
            (Number, "2.5E-3"),
            (Number, "0xFF"),
            (Number, "1_000"),
            (Name, "t"),
            (Punctuation, "."),
            (Number, "5"),
        ]
    );
}

#[test]
fn comments_nest() {
    assert_eq!(
        tokens("a -- one\nb /* x /* y */ z */ c /**/ d"),
        [
            (Name, "a"),
            (Comment, "-- one"),
            (Name, "b"),
            (Comment, "/* x /* y */ z */"),
            (Name, "c"),
            (Comment, "/**/"),
            (Name, "d"),
        ]
    );
}

#[test]
fn operators_and_parameters() {
    assert_eq!(
        tokens("a::int <> b || c ->> 'k' >= :p @v @@g $1 ? x:=1 a-1 b--c"),
        [
            (Name, "a"),
            (Operator, "::"),
            (Type, "int"),
            (Operator, "<>"),
            (Name, "b"),
            (Operator, "||"),
            (Name, "c"),
            (Operator, "->>"),
            (String, "'k'"),
            (Operator, ">="),
            (Parameter, ":p"),
            (Parameter, "@v"),
            (Parameter, "@@g"),
            (Parameter, "$1"),
            (Parameter, "?"),
            (Name, "x"),
            (Operator, ":="),
            (Number, "1"),
            (Name, "a"),
            (Operator, "-"),
            (Number, "1"),
            (Name, "b"),
            (Comment, "--c"),
        ]
    );
}

#[test]
fn jinja_blocks_are_templates() {
    assert_eq!(
        tokens("select * from {{ ref('orders') }} {%- if x %}where 1{% endif -%} {# note #}"),
        [
            (Keyword, "select"),
            (Operator, "*"),
            (Keyword, "from"),
            (Template, "{{ ref('orders') }}"),
            (Template, "{%- if x %}"),
            (Keyword, "where"),
            (Number, "1"),
            (Template, "{% endif -%}"),
            (Template, "{# note #}"),
        ]
    );
    // A struct literal is punctuation.
    assert_eq!(tokens("{'a': 1}")[0], (Punctuation, "{"));
}

#[test]
fn unterminated_input_runs_to_the_end() {
    for (code, kind) in [
        ("'abc", String),
        ("\"abc", QuotedName),
        ("`abc", QuotedName),
        ("[abc", QuotedName),
        ("/* a /* b */", Comment),
        ("{{ ref(", Template),
        ("{% if", Template),
        ("$tag$ body", String),
        ("E'\\", String),
        ("'''", String),
    ] {
        let all = tokens(code);
        assert_eq!(all, [(kind, code)], "{code:?}");
    }
}

/// A small deterministic generator (xorshift), so the property needs no
/// dependency.
fn random_inputs() -> impl Iterator<Item = std::string::String> {
    const ALPHABET: &[&str] = &[
        "'", "\"", "`", "[", "]", "{", "}", "{{", "}}", "{%", "%}", "{#", "#}", "/*", "*/", "--",
        "$", "$$", "$a$", "\\", "\n", " ", "\t", "é", "日本", "😀", "e", "E", "x", "0", "1", ".",
        ":", "::", "@", "?", "-", "+", "<", ">", "=", "!", "|", "select", "from", "(", ")", ",",
        ";", "\r", "\0",
    ];
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    (0..3000).map(move |_| {
        let len = (next() % 40) as usize;
        (0..len)
            .map(|_| ALPHABET[(next() % ALPHABET.len() as u64) as usize])
            .collect()
    })
}

#[test]
fn random_input_tokenizes_and_highlights_without_panicking() {
    let highlighter = SqlHighlighter::new();
    for code in random_inputs() {
        let all = tokenize(&code);
        let mut at = 0;
        for token in &all {
            assert_eq!(token.range.start, at, "{code:?}");
            assert!(token.range.end > at, "{code:?}");
            assert!(code.is_char_boundary(token.range.end), "{code:?}");
            at = token.range.end;
        }
        assert_eq!(at, code.len(), "{code:?}");

        let highlighted = highlighter
            .highlight(&code, Some("sql"), "ansi_dark")
            .unwrap();
        let sources: Vec<&str> = code.split('\n').collect();
        assert_eq!(highlighted.lines.len(), sources.len(), "{code:?}");
        for (line, source) in highlighted.lines.iter().zip(&sources) {
            let mut end = 0;
            for span in &line.spans {
                assert!(span.range.start >= end && span.range.start < span.range.end);
                assert!(span.range.end <= source.len(), "{code:?}");
                assert!(source.is_char_boundary(span.range.start));
                assert!(source.is_char_boundary(span.range.end));
                end = span.range.end;
            }
        }
        assert_eq!(
            highlight(&code, "monokai").plain(),
            Text::new(code.as_str()).plain()
        );
    }
}

#[test]
fn spans_split_at_line_breaks() {
    let highlighted = SqlHighlighter::new()
        .highlight("select '\nx'", Some("SQL"), "ansi_dark")
        .unwrap();
    assert_eq!(highlighted.lines.len(), 2);
    let yellow = Style::parse("yellow").unwrap();
    assert_eq!(highlighted.lines[0].spans[1].range, 7..8);
    assert_eq!(highlighted.lines[0].newline_style, Some(yellow.clone()));
    assert_eq!(highlighted.lines[1].spans[0].range, 0..2);
    assert_eq!(highlighted.lines[1].spans[0].style, yellow);
}

#[test]
fn languages_themes_and_fallback() {
    let sql = SqlHighlighter::new();
    for name in [
        "sql",
        "postgresql",
        "mysql",
        "sqlite",
        "bigquery",
        "snowflake",
        "duckdb",
    ] {
        assert!(sql.languages().contains(&name.to_string()), "{name}");
        let out = sql.highlight("select", Some(name), "ansi_dark").unwrap();
        assert_eq!(out.lines[0].spans.len(), 1, "{name}");
    }
    // Another language is plain text without a fallback.
    let out = sql
        .highlight("select", Some("python"), "ansi_dark")
        .unwrap();
    assert!(out.lines[0].spans.is_empty());
    // An unknown theme is monokai.
    let out = sql.highlight("select", Some("sql"), "dracula").unwrap();
    assert_eq!(
        out.lines[0].spans[0].style.color(),
        Style::parse("#66d9ef").unwrap().color()
    );
    assert!(out.background.is_some());
    assert_eq!(
        sql.language_for_path(Path::new("models/orders.SQL")),
        Some("sql".to_string())
    );
    assert_eq!(sql.language_for_path(Path::new("a.py")), None);

    /// Bolds every line, to show the fallback was used.
    struct Bold;
    impl CodeHighlighter for Bold {
        fn highlight(
            &self,
            code: &str,
            _: Option<&str>,
            _: &str,
        ) -> Result<HighlightedCode, HighlightError> {
            Ok(HighlightedCode {
                lines: code
                    .split('\n')
                    .map(|line| HighlightedLine {
                        spans: vec![HighlightSpan {
                            range: 0..line.len(),
                            style: Style::parse("bold").unwrap(),
                        }],
                        newline_style: None,
                    })
                    .collect(),
                ..Default::default()
            })
        }
        fn default_theme(&self) -> &str {
            "bold"
        }
        fn themes(&self) -> Vec<std::string::String> {
            vec!["bold".into()]
        }
        fn languages(&self) -> Vec<std::string::String> {
            vec!["python".into()]
        }
    }
    let both = SqlHighlighter::with_fallback(Arc::new(Bold));
    let out = both.highlight("x = 1", Some("python"), "bold").unwrap();
    assert_eq!(out.lines[0].spans[0].style, Style::parse("bold").unwrap());
    assert!(both.themes().contains(&"bold".to_string()));
    assert!(both.languages().contains(&"python".to_string()));
}

#[test]
fn the_plugin_registers_sql() {
    let mut registry = crate::registry::ExtensionRegistry::new();
    registry.add_plugin(&SqlPlugin).unwrap();
    assert!(registry.code_highlighter("sql").is_some());
}

#[cfg(feature = "syntax")]
mod with_syntax {
    use super::*;
    use rich::protocol::{CodeHighlighting, ConsoleCodeHighlighting};
    use rich::{ColorSystem, Console};

    fn console() -> Console {
        let mut console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Standard))
            .width(40)
            .build();
        console.set_code_highlighting(Some(CodeHighlighting {
            highlighter: SqlHighlighter::shared(),
            theme: Some("ansi_dark".into()),
        }));
        console
    }

    #[test]
    fn a_sql_code_block_is_highlighted() {
        let out = console().render_to_string(&rich::Syntax::new("SELECT 'x' -- c", "sql"));
        assert!(out.contains("\x1b[94mSELECT"), "{out:?}");
        assert!(out.contains("\x1b[33m'x'"), "{out:?}");
        assert!(out.contains("\x1b[2m-- c"), "{out:?}");
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn a_sql_markdown_fence_is_highlighted() {
        let markdown = rich::markdown::Markdown::new("```sql\nselect 1 from {{ ref('t') }}\n```\n");
        let out = console().render_to_string(&markdown);
        assert!(out.contains("\x1b[94mselect"), "{out:?}");
        assert!(out.contains("\x1b[96m{{ ref('t') }}"), "{out:?}");
    }
}
