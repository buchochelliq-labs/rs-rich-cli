use rich::{Console, Renderable};
use rich_ext::ansi_explain::{
    escape_visible, explain, explain_bytes, sgr_effects, ExplanationView, Spanned, StringKind,
    Terminator, Token, ViewMode,
};

fn plain(r: &dyn Renderable, width: usize) -> String {
    let c = Console::builder()
        .width(width)
        .no_color(true)
        .color_system(None)
        .build();
    c.segments_to_string(&r.rich_render(&c, &c.options()))
}

/// `(kind, meaning)` for every token of `input`.
fn meanings(input: &str) -> Vec<(&'static str, String)> {
    explain(input)
        .tokens
        .iter()
        .map(|t| (t.token.kind(), t.token.meaning()))
        .collect()
}

fn sgr(params: &str) -> Vec<String> {
    sgr_effects(params)
        .into_iter()
        .map(|e| e.description)
        .collect()
}

#[test]
fn sgr_is_covered_fully() {
    assert_eq!(sgr(""), ["reset"]);
    assert_eq!(sgr("0"), ["reset"]);
    assert_eq!(sgr("01;34"), ["bold on", "fg blue"]);
    assert_eq!(
        sgr("1;2;3;4;5;7;8;9"),
        [
            "bold on",
            "dim on",
            "italic on",
            "underline on",
            "slow blink on",
            "reverse on",
            "conceal on",
            "strikethrough on"
        ]
    );
    assert_eq!(
        sgr("21;22;23;24;25;27;28;29;53;55"),
        [
            "double underline on",
            "bold and dim off",
            "italic off",
            "underline off",
            "blink off",
            "reverse off",
            "conceal off",
            "strikethrough off",
            "overline on",
            "overline off"
        ]
    );
    assert_eq!(
        sgr("30;47;90;107;39;49"),
        [
            "fg black",
            "bg white",
            "fg bright black",
            "bg bright white",
            "fg default",
            "bg default"
        ]
    );
    assert_eq!(sgr("38;5;208"), ["fg 256-colour 208 (#ff8700)"]);
    assert_eq!(sgr("48;5;9"), ["bg 256-colour 9 (bright red)"]);
    assert_eq!(sgr("38;2;255;135;0;1"), ["fg #ff8700", "bold on"]);
    assert_eq!(sgr("58;2;0;0;255"), ["underline colour #0000ff"]);
    assert_eq!(sgr("59"), ["underline colour default"]);
    // Colon sub-parameters: with and without the colour-space id.
    assert_eq!(sgr("38:2::255:135:0"), ["fg #ff8700"]);
    assert_eq!(sgr("38:2:255:135:0"), ["fg #ff8700"]);
    assert_eq!(sgr("48:5:208"), ["bg 256-colour 208 (#ff8700)"]);
    assert_eq!(sgr("58:2::1:2:3"), ["underline colour #010203"]);
    assert_eq!(
        sgr("4:0;4:1;4:2;4:3;4:4;4:5;4:9"),
        [
            "underline off",
            "underline on (single)",
            "underline on (double)",
            "underline on (curly)",
            "underline on (dotted)",
            "underline on (dashed)",
            "unknown underline style 4:9"
        ]
    );
    // Malformed and unknown parameters are described, never dropped.
    assert_eq!(sgr("38;5"), ["incomplete 38 (expected 38;5;n)"]);
    assert_eq!(sgr("38;2;1;2"), ["incomplete 38 (expected 38;2;r;g;b)"]);
    assert_eq!(
        sgr("38;9;1"),
        ["incomplete 38 (expected ;5 or ;2 colour form)", "bold on"]
    );
    assert_eq!(sgr("38:2::1"), ["incomplete 38 (expected 38:2::r:g:b)"]);
    assert_eq!(sgr("38;2;300;0;0"), ["fg rgb(300,0,0) (out of range)"]);
    assert_eq!(sgr("99;x"), ["unknown SGR 99", "invalid parameter \"x\""]);
    assert_eq!(sgr("12"), ["alternative font 2"]);
}

#[test]
fn csi_osc_esc_and_strings() {
    assert_eq!(
        meanings("\x1b[?25l\x1b[?1049h\x1b[?2004h\x1b[?25h\x1b[2J\x1b[3;4H\x1b[K\x1b[1K\x1b[5S\x1b[2T\x1b[3A\x1b[B\x1b[10G\x1b[2 q\x1b[?1000;1006h"),
        [
            ("CSI", "hide cursor (DECRST)".into()),
            ("CSI", "enable alternate screen with saved cursor (DECSET)".into()),
            ("CSI", "enable bracketed paste (DECSET)".into()),
            ("CSI", "show cursor (DECSET)".into()),
            ("CSI", "erase entire screen".into()),
            ("CSI", "cursor to row 3, column 4".into()),
            ("CSI", "erase from cursor to end of line".into()),
            ("CSI", "erase from start of line to cursor".into()),
            ("CSI", "scroll up 5 lines".into()),
            ("CSI", "scroll down 2 lines".into()),
            ("CSI", "cursor up 3".into()),
            ("CSI", "cursor down 1".into()),
            ("CSI", "cursor to column 10".into()),
            ("CSI", "cursor style: steady block".into()),
            (
                "CSI",
                "enable mouse click tracking, enable SGR mouse encoding (DECSET)".into()
            ),
        ]
    );
    assert_eq!(
        meanings("\x1b[5y\x1b[>1u\x1b[6n\x1b[c"),
        [
            ("CSI", "unrecognised control sequence (CSI 5y)".into()),
            ("CSI", "push keyboard enhancement flags 1".into()),
            ("CSI", "request cursor position".into()),
            ("CSI", "request primary device attributes (DA1)".into()),
        ]
    );
    // OSC with BEL and ST terminators, 7-bit and 8-bit.
    let e = explain("\x1b]8;id=1;https://a.b\x07link\x1b]8;;\x1b\\\u{9d}2;t\u{9c}");
    match &e.tokens[0].token {
        Token::Osc {
            code,
            terminator,
            meaning,
            ..
        } => {
            assert_eq!((*code, *terminator), (Some(8), Terminator::Bel));
            assert_eq!(
                meaning,
                "open hyperlink to https://a.b (id=1) (BEL-terminated)"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        e.tokens[2].token,
        Token::Osc {
            terminator: Terminator::St,
            ..
        }
    ));
    assert_eq!(
        e.tokens[2].token.meaning(),
        "close hyperlink (ST-terminated)"
    );
    assert_eq!(
        e.tokens[3].token.meaning(),
        "set window title to \"t\" (ST-terminated)"
    );
    assert_eq!(e.visible_text, "link");
    assert_eq!(
        meanings("\x1b]0;title\x07\x1b]133;A\x07\x1b]133;D;1\x07\x1b]52;c;aGVsbG8=\x07\x1b]52;c;?\x07\x1b]4;1;rgb:ff/00/00;2;?\x07\x1b]10;?\x07\x1b]11;#000\x1b\\\x1b]7;file://h/p\x07\x1b]1337;File=inline=1:AAAA\x07\x1b]999;x\x07"),
        [
            ("OSC", "set window and icon title to \"title\" (BEL-terminated)".into()),
            ("OSC", "shell integration: prompt start (BEL-terminated)".into()),
            ("OSC", "shell integration: command finished (exit status 1) (BEL-terminated)".into()),
            ("OSC", "set clipboard (c) to 8 bytes of base64 (BEL-terminated)".into()),
            ("OSC", "query clipboard (c) (BEL-terminated)".into()),
            ("OSC", "set palette colour 1 to rgb:ff/00/00, query palette colour 2 (BEL-terminated)".into()),
            ("OSC", "query default foreground colour (BEL-terminated)".into()),
            ("OSC", "set default background colour to #000 (ST-terminated)".into()),
            ("OSC", "report working directory file://h/p (BEL-terminated)".into()),
            ("OSC", "iTerm2 inline file (inline=1), 4 bytes of base64 (BEL-terminated)".into()),
            ("OSC", "unrecognised OSC 999 (BEL-terminated)".into()),
        ]
    );
    assert_eq!(
        meanings("\x1b7\x1b8\x1bc\x1b=\x1b>\x1b(0\x1b(B\x1b#8\x1bM\x1b%G\x1bQ"),
        [
            ("ESC", "save cursor (DECSC)".into()),
            ("ESC", "restore cursor (DECRC)".into()),
            ("ESC", "full reset (RIS)".into()),
            ("ESC", "application keypad mode (DECKPAM)".into()),
            ("ESC", "numeric keypad mode (DECKPNM)".into()),
            (
                "ESC",
                "G0 character set: DEC special graphics (line drawing)".into()
            ),
            ("ESC", "G0 character set: US ASCII".into()),
            ("ESC", "screen alignment test (DECALN)".into()),
            ("ESC", "reverse index: move up one line (RI)".into()),
            ("ESC", "select UTF-8 character set".into()),
            ("ESC", "unrecognised escape sequence (ESC Q)".into()),
        ]
    );
    // DCS payloads are summarised, not dumped.
    let sixel = format!(
        "\x1bP0;1;0q\"1;1;8;8#0;2;100;0;0#0{}\x1b\\",
        "~".repeat(500)
    );
    let e = explain(&sixel);
    assert_eq!(e.tokens.len(), 1);
    match &e.tokens[0].token {
        Token::Dcs {
            kind,
            data_len,
            meaning,
            ..
        } => {
            assert_eq!(*kind, StringKind::Dcs);
            assert_eq!(*data_len, 522);
            assert_eq!(meaning, "sixel image, 522 bytes of data (params 0;1;0)");
        }
        other => panic!("{other:?}"),
    }
    let table = plain(&ExplanationView::new(&e).show_visible(false), 140);
    assert!(
        table.contains("ESCP0;1;0q\"1;1;8;8#0;2;100;0;0#0~~~~~~~~… (532 bytes)"),
        "{table}"
    );
    assert!(!table.contains(&"~".repeat(50)));
    assert_eq!(
        meanings("\x1bP$qm\x1b\\\x1bP+q544e\x1b\\\x1bPtmux;\x1b\x1b[1m\x1b\\\x1b_Ga=T,f=100;AAAA\x1b\\\x1b^pm\x1b\\"),
        [
            ("DCS", "request setting \"m\" (DECRQSS)".into()),
            ("DCS", "request terminfo capabilities 544e (XTGETTCAP)".into()),
            ("DCS", "tmux passthrough, 5 bytes".into()),
            ("APC", "kitty graphics command (a=T,f=100), 4 bytes of payload".into()),
            ("PM", "privacy message, 2 bytes".into()),
        ]
    );
}

#[test]
fn controls_c1_and_invalid_sequences() {
    assert_eq!(
        meanings("a\tb\x08c\rd\x00e\x7f\x07"),
        [
            ("text", "1 character".into()),
            ("control", "horizontal tab".into()),
            ("text", "1 character".into()),
            ("control", "backspace".into()),
            ("text", "1 character".into()),
            ("control", "carriage return".into()),
            ("text", "1 character".into()),
            ("control", "null (ignored)".into()),
            ("text", "1 character".into()),
            ("control", "delete (ignored)".into()),
            ("control", "bell".into()),
        ]
    );
    assert_eq!(explain("a\tb\x08c\r\nd").visible_text, "a\tbc\nd");
    // Truncated and interrupted sequences.
    for (input, reason) in [
        ("\x1b", "truncated escape sequence at end of input"),
        ("\x1b[1", "truncated control sequence (CSI) at end of input"),
        ("\x1b[", "truncated control sequence (CSI) at end of input"),
        ("\x1b]8;;http", "unterminated OSC at end of input"),
        ("\x1bPq#0~", "unterminated DCS at end of input (4 bytes)"),
        ("\x1b(", "truncated escape sequence at end of input"),
    ] {
        let e = explain(input);
        assert_eq!(
            e.tokens,
            [Spanned {
                offset: 0,
                len: input.len(),
                token: Token::Invalid {
                    raw: input.into(),
                    reason: reason.into()
                }
            }],
            "{input:?}"
        );
    }
    // An interrupted CSI stops before the interrupting control, which is kept.
    assert_eq!(
        meanings("\x1b[12;3\x07m"),
        [
            (
                "invalid",
                "control sequence (CSI) interrupted by BEL".into()
            ),
            ("control", "bell".into()),
            ("text", "1 character".into()),
        ]
    );
    assert_eq!(
        meanings("\x1b]0;x\x1b[1m"),
        [
            ("invalid", "OSC interrupted by ESC without ST".into()),
            ("SGR", "bold on".into()),
        ]
    );
    assert_eq!(
        meanings("\x1bPq#0\x1b[1m"),
        [
            (
                "invalid",
                "unterminated DCS interrupted by ESC (3 bytes)".into()
            ),
            ("SGR", "bold on".into()),
        ]
    );
    // 8-bit C1 controls, as characters and as raw bytes.
    let e = explain("\u{9b}1mX\u{9b}0m\u{85}\u{8d}\u{9c}");
    assert_eq!(
        e.tokens
            .iter()
            .map(|t| t.token.meaning())
            .collect::<Vec<_>>(),
        [
            "bold on",
            "1 character",
            "reset",
            "next line (8-bit)",
            "reverse index (8-bit)",
            "string terminator with no string (8-bit)"
        ]
    );
    let e = explain_bytes(b"\x9b31mred\x9b0m \xff\x1b[1m");
    assert_eq!(e.visible_text, "red \u{fffd}");
    assert_eq!(e.tokens[0].token.meaning(), "fg red");
    assert_eq!(escape_visible(&e.tokens[0].token.raw()), "<CSI>31m");
    assert_eq!(e.tokens.last().unwrap().token.meaning(), "bold on");
    // Offsets are byte offsets into the (decoded) input.
    let e = explain("é\x1b[1mx");
    assert_eq!((e.tokens[1].offset, e.tokens[1].len), (2, 4));
}

#[test]
fn real_tool_output() {
    let ls = explain(include_str!("fixtures/ansi/ls.ansi"));
    assert_eq!(
        ls.visible_text,
        "Cargo.toml\nLICENSE\nREADME.md\nexamples/\nsrc/\ntests/\n"
    );
    assert!(ls.invalid().next().is_none());
    assert_eq!(
        ls.escapes()
            .filter(|t| t.token.kind() == "SGR")
            .map(|t| t.token.meaning())
            .collect::<Vec<_>>(),
        [
            "reset",
            "bold on, fg blue",
            "reset",
            "bold on, fg blue",
            "reset",
            "bold on, fg blue",
            "reset"
        ]
    );

    let git = explain(include_str!("fixtures/ansi/git-log.ansi"));
    assert!(git
        .visible_text
        .starts_with("c4461c9 n1ckyb Merge pull request #506"));
    assert_eq!(
        git.escapes().map(|t| t.token.meaning()).collect::<Vec<_>>(),
        [
            "fg yellow",
            "reset",
            "bold on, fg blue",
            "reset",
            "line feed (newline)"
        ]
    );

    let grep = explain(include_str!("fixtures/ansi/grep.ansi"));
    assert_eq!(
        grep.visible_text,
        "23:pub mod a11y;\n24:pub mod ansi_explain;\n25:pub mod capabilities;\n"
    );
    let first_line: Vec<String> = grep
        .tokens
        .iter()
        .take_while(|t| !matches!(t.token, Token::Control { byte: b'\n', .. }))
        .filter(|t| t.token.kind() != "text")
        .map(|t| t.token.meaning())
        .collect();
    assert_eq!(
        first_line,
        [
            "fg green",
            "erase from cursor to end of line",
            "reset",
            "erase from cursor to end of line",
            "fg cyan",
            "erase from cursor to end of line",
            "reset",
            "erase from cursor to end of line",
            "bold on, fg red",
            "erase from cursor to end of line",
            "reset",
            "erase from cursor to end of line",
        ]
    );
    // Rich's own output round-trips.
    let c = Console::builder()
        .force_terminal(true)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .build();
    let out = c.render_str_to_string("[bold #ff8700 on blue]hi[/] [link=https://x.y]x[/link]");
    let e = explain(&out);
    assert_eq!(e.visible_text, "hi x");
    assert!(e
        .escapes()
        .any(|t| t.token.meaning() == "bold on, fg #ff8700, bg blue"));
    assert!(e.escapes().any(|t| t
        .token
        .meaning()
        .starts_with("open hyperlink to https://x.y")));
}

#[test]
fn explanation_views() {
    let e = explain("\x1b[1;38;5;208mhi\x1b[0m\x1b]8;;https://a.b\x07link\x1b]8;;\x1b\\\r\n");
    assert_eq!(
        plain(&ExplanationView::new(&e), 100),
        "\
┏━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ Offset ┃ Raw                     ┃ Kind    ┃ Meaning                                        ┃
┡━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩
│ 0      │ ESC[1;38;5;208m         │ SGR     │ bold on, fg 256-colour 208 (#ff8700)           │
│ 13     │ hi                      │ text    │ 2 characters                                   │
│ 15     │ ESC[0m                  │ SGR     │ reset                                          │
│ 19     │ ESC]8;;https://a.b<BEL> │ OSC     │ open hyperlink to https://a.b (BEL-terminated) │
│ 36     │ link                    │ text    │ 4 characters                                   │
│ 40     │ ESC]8;;ESC\\             │ OSC     │ close hyperlink (ST-terminated)                │
│ 47     │ <CR>                    │ control │ carriage return                                │
│ 48     │ <LF>                    │ control │ line feed (newline)                            │
└────────┴─────────────────────────┴─────────┴────────────────────────────────────────────────┘
Visible text:
hilink
"
    );
    let escapes = plain(
        &ExplanationView::new(&e)
            .escapes_only(true)
            .show_visible(false),
        100,
    );
    assert!(!escapes.contains(" text ") && !escapes.contains("Visible text"));
    assert_eq!(escapes.lines().count(), 3 + 6 + 1);

    let view = ExplanationView::new(&e).mode(ViewMode::Inline);
    assert_eq!(
        view.inline_text(false),
        "⟨bold on, fg 256-colour 208 (#ff8700)⟩hi⟨reset⟩⟨open hyperlink to https://a.b (BEL-terminated)⟩link⟨close hyperlink (ST-terminated)⟩⟨CR⟩\n"
    );
    assert_eq!(
        view.inline_text(true),
        view.inline_text(false).replace('⟨', "<").replace('⟩', ">")
    );
    let ascii = Console::builder()
        .width(200)
        .ascii_only(true)
        .no_color(true)
        .build();
    let inline = ascii.segments_to_string(&view.rich_render(&ascii, &ascii.options()));
    assert!(
        inline.starts_with("<bold on, fg 256-colour 208 (#ff8700)>hi<reset>"),
        "{inline}"
    );
}

#[cfg(feature = "serde")]
#[test]
fn explanation_serializes() {
    let e = explain("\x1b[1mx\x07");
    let json = serde_json::to_value(&e).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "tokens": [
                {"offset": 0, "len": 4, "token": {"type": "sgr", "value": {"raw": "\u{1b}[1m", "params": "1",
                    "effects": [{"code": "1", "description": "bold on"}]}}},
                {"offset": 4, "len": 1, "token": {"type": "text", "value": "x"}},
                {"offset": 5, "len": 1, "token": {"type": "control", "value": {"byte": 7, "name": "BEL"}}}
            ],
            "visible_text": "x"
        })
    );
}
