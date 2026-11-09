use super::*;

fn atom<'a>(text: &'a str, width: u16, style: Option<&'a Style>) -> Vec<PremeasuredCell<'a>> {
    let mut cells = vec![
        PremeasuredCell {
            text: "",
            width: 0,
            style
        };
        width as usize
    ];
    cells[0] = PremeasuredCell { text, width, style };
    cells
}

fn imported(text: &str, width: u16, columns: u16) -> Screen {
    let mut screen = Screen::new(columns, 1);
    screen
        .write_premeasured(Rect::new(0, 0, width, 1), &atom(text, width, None))
        .unwrap();
    screen
}

#[test]
fn premeasured_literal_atoms_survive_input_drop() {
    let long = format!("a{}", "\u{903}".repeat(300));
    for (text, width) in [
        ("⌚\u{fe0e}", 1),
        ("🇺🇸", 2),
        ("क्ष", 2),
        ("\u{17d8}", 3),
        (long.as_str(), 301),
    ] {
        let mut screen = Screen::new(width + 1, 1);
        {
            let owned = text.to_owned();
            let style = Style::parse("bold italic red")
                .unwrap()
                .with_link("https://example.test/safe");
            let mut cells = atom(&owned, width, Some(&style));
            cells.push(PremeasuredCell {
                text: "X",
                width: 1,
                style: None,
            });
            assert_eq!(
                screen.write_premeasured(screen.area(), &cells).unwrap(),
                screen.area()
            );
        }
        assert_eq!(
            (screen.cell(0, 0).text(), screen.cell(0, 0).width),
            (text, width)
        );
        let style = screen.style(screen.cell(0, 0).style).unwrap();
        assert_eq!(style.bold(), Some(true));
        assert_eq!(style.italic(), Some(true));
        assert_eq!(style.link(), Some("https://example.test/safe"));
        for x in 1..width {
            assert_eq!((screen.cell(x, 0).text(), screen.cell(x, 0).width), ("", 0));
            assert_eq!(screen.cell(x, 0).style, screen.cell(0, 0).style);
        }
        assert_eq!(
            (screen.cell(width, 0).text(), screen.cell(width, 0).width),
            ("X", 1)
        );
    }
    assert_eq!(std::mem::size_of::<Cell>(), 12);
    let width: u8 = 2;
    assert_eq!(Cell::new("字", width, StyleId::NONE).width, 2u16);
}

#[test]
fn premeasured_validation_is_atomic_even_outside_clip() {
    use PremeasuredCellError::*;
    let a = PremeasuredCell {
        text: "a",
        width: 1,
        style: None,
    };
    let continuation = PremeasuredCell {
        text: "",
        width: 0,
        style: None,
    };
    let bold = Style::parse("bold").unwrap();
    let bad_link = bold.clone().with_link("https://example.test/\u{9c}");
    let cases = [
        (vec![PremeasuredCell { text: "", ..a }], EmptyLeader),
        (vec![continuation], OrphanContinuation),
        (
            vec![PremeasuredCell { width: 0, ..a }],
            NonemptyContinuation,
        ),
        (vec![PremeasuredCell { width: 2, ..a }], SpanCrossesRow),
        (
            vec![PremeasuredCell { width: 2, ..a }, a],
            MissingContinuation,
        ),
        (
            vec![
                PremeasuredCell { width: 2, ..a },
                PremeasuredCell {
                    text: "x",
                    ..continuation
                },
            ],
            NonemptyContinuation,
        ),
        (
            vec![
                PremeasuredCell { width: 2, ..a },
                PremeasuredCell {
                    style: Some(&bold),
                    ..continuation
                },
            ],
            ContinuationStyle,
        ),
        (vec![PremeasuredCell { text: "\x1b", ..a }], TextControl),
        (
            vec![PremeasuredCell {
                style: Some(&bad_link),
                ..a
            }],
            LinkControl,
        ),
    ];
    for (bad, reason) in cases {
        let mut screen = imported("wide", 3, 4);
        screen.scroll_up(Rect::new(0, 0, 4, 1), 0);
        let before = screen.clone();
        let error = screen
            .write_premeasured(Rect::new(4, 0, bad.len() as u16, 1), &bad)
            .unwrap_err();
        assert!(
            matches!(error, PremeasuredError::Cell { reason: got, .. } if got == reason),
            "{error:?}"
        );
        assert_eq!(screen.cells, before.cells);
        assert_eq!(screen.style_count(), before.style_count());
        assert_eq!(screen.scrolled(), before.scrolled());
    }
    let mut screen = imported("x", 1, 1);
    assert_eq!(
        screen
            .write_premeasured(Rect::new(0, 0, 2, 2), &[a])
            .unwrap_err(),
        PremeasuredError::Shape {
            expected: 4,
            actual: 1
        }
    );
    for c in ['\0', '\n', '\r', '\t', '\x7f', '\u{80}', '\u{9f}'] {
        let text = c.to_string();
        assert!(
            screen
                .write_premeasured(screen.area(), &[PremeasuredCell { text: &text, ..a }])
                .is_err()
        );
    }
    let before = screen.cells.clone();
    assert!(
        screen
            .write_premeasured(Rect::new(0, 0, 0, 8), &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(before, screen.cells);
}

#[test]
fn premeasured_styles_compare_by_value_and_stride_survives_clipping() {
    let style = Style::parse("bold").unwrap();
    let same = style.clone();
    let cells = [
        PremeasuredCell {
            text: "opaque",
            width: 2,
            style: Some(&style),
        },
        PremeasuredCell {
            text: "",
            width: 0,
            style: Some(&same),
        },
        PremeasuredCell {
            text: "A",
            width: 1,
            style: None,
        },
        PremeasuredCell {
            text: "B",
            width: 1,
            style: None,
        },
    ];
    let mut screen = Screen::new(1, 2);
    screen
        .write_premeasured(Rect::new(0, 0, 2, 2), &cells)
        .unwrap();
    assert_eq!(
        (screen.cell(0, 0).text(), screen.cell(0, 0).width),
        (" ", 1)
    );
    assert_eq!(screen.cell(0, 1).text(), "A");
    assert_eq!(screen.style(screen.cell(0, 0).style), Some(&style));
}

#[test]
fn premeasured_clips_whole_atoms_and_repairs_every_continuation() {
    for width in [3, 301] {
        let source = imported("opaque", width, width);
        for cut in 1..width {
            let mut clipped = Screen::new(cut, 1);
            clipped
                .write_premeasured(Rect::new(0, 0, width, 1), &atom("opaque", width, None))
                .unwrap();
            assert!(
                clipped
                    .cells
                    .iter()
                    .all(|c| c.text() == " " && c.width == 1)
            );
            let mut dest = Screen::new(width, 1);
            dest.blit(&source, Rect::new(cut, 0, width - cut, 1), 0, 0);
            assert!(dest.cells.iter().all(|c| c.text() == " " && c.width == 1));
            for operation in 0..3 {
                let mut changed = source.clone();
                match operation {
                    0 => changed.put(cut, 0, 'X', StyleId::NONE),
                    1 => changed.clear(Rect::new(cut, 0, 1, 1)),
                    _ => changed
                        .write_lines(Rect::new(cut, 0, 1, 1), &[vec![Segment::new("X", None)]]),
                }
                assert!(changed.cells.iter().all(|c| c.width == 1));
                for x in 0..width {
                    assert_eq!(
                        changed.cell(x, 0).text(),
                        if x == cut && operation != 1 { "X" } else { " " }
                    );
                }
            }
            let mut changed = source.clone();
            let damage = changed
                .write_premeasured(Rect::new(cut, 0, 1, 1), &atom("X", 1, None))
                .unwrap();
            assert_eq!(damage, source.area());
            assert!(changed.cells.iter().all(|c| c.width == 1));
        }
    }
}

#[test]
fn premeasured_full_u16_bounds_and_overflowing_origin() {
    let cells = atom("opaque", u16::MAX, None);
    let mut screen = Screen::new(u16::MAX, 1);
    screen.write_premeasured(screen.area(), &cells).unwrap();
    assert_eq!(screen.cell(0, 0).width, u16::MAX);
    assert_eq!(screen.cell(u16::MAX - 1, 0).width, 0);
    let mut clipped = Screen::new(u16::MAX, 1);
    clipped
        .write_premeasured(Rect::new(u16::MAX - 1, 0, u16::MAX, 1), &cells)
        .unwrap();
    assert_eq!(clipped.cell(u16::MAX - 1, 0).text(), " ");
    screen.put(u16::MAX - 1, 0, 'X', StyleId::NONE);
    assert!(screen.cells.iter().all(|c| c.width == 1));
}

#[test]
fn premeasured_provenance_survives_copy_scroll_and_restyle() {
    let source = imported("opaque", 3, 3);
    let mut screen = Screen::new(3, 2);
    screen.blit(&source, source.area(), 0, 1);
    screen.scroll_up(screen.area(), 1);
    screen.restyle(2, 0, &Style::parse("bold").unwrap());
    screen.compact_styles();
    for x in 0..3 {
        assert!(screen.cell(x, 0).premeasured());
        assert_eq!(screen.cell(x, 0).width, if x == 0 { 3 } else { 0 });
    }
    assert_ne!(
        imported("A", 1, 1).cell(0, 0),
        &Cell::new("A", 1, StyleId::NONE)
    );
}

#[test]
fn premeasured_damage_closes_old_and_new_spans_and_sent_agrees() {
    for strategy in PaintStrategy::ALL {
        let mut screen = imported("opaque", 301, 305);
        let mut painter = Painter::new(None);
        painter.set_strategy(strategy);
        painter.try_paint(&screen, &[]).unwrap();
        screen.restyle(299, 0, &Style::parse("bold").unwrap());
        let out = painter
            .try_paint(&screen, &[Rect::new(299, 0, 1, 1)])
            .unwrap();
        assert!(out.contains("opaque"), "{strategy}: {out:?}");
        assert_eq!(painter.previous.as_ref().unwrap().cells, screen.cells);
        assert!(painter.try_paint(&screen, &[]).unwrap().is_empty());
        assert!(painter.try_paint(&screen, &[]).unwrap().is_empty());
        screen
            .write_premeasured(Rect::new(299, 0, 3, 1), &atom("new", 3, None))
            .unwrap();
        painter
            .try_paint(&screen, &[Rect::new(299, 0, 1, 1)])
            .unwrap();
        assert_eq!(painter.previous.as_ref().unwrap().cells, screen.cells);
        let out = painter
            .try_repaint(&screen, &[Rect::new(301, 0, 1, 1)])
            .unwrap();
        assert!(out.contains("new"));
        assert_eq!(painter.previous.as_ref().unwrap().cells, screen.cells);
    }
}

#[test]
fn premeasured_fullscreen_positions_and_modes_are_explicit() {
    for (text, width) in [("⌚\u{fe0e}", 1), ("🇺🇸", 2), ("क्ष", 2), ("\u{17d8}", 3)] {
        let mut screen = imported(text, width, width + 1);
        screen.put(width, 0, 'X', StyleId::NONE);
        let mut painter = Painter::new(None);
        let out = painter.try_paint(&screen, &[]).unwrap();
        assert!(out.starts_with("\x1b[?7l"));
        assert!(out.ends_with("\x1b[?7h"));
        assert!(
            out.contains(&format!("{text}\x1b[1;{}H", width + 1)),
            "{out:?}"
        );
        assert_eq!(painter.stats().bytes(), out.len());
        let mut terminal = vt100::Parser::new(1, width + 4, 0);
        terminal.process(out.as_bytes());
        assert_eq!(terminal.screen().cell(0, width).unwrap().contents(), "X");
        assert!(
            painter
                .try_paint(&screen, &[screen.area()])
                .unwrap()
                .is_empty()
        );
    }
    let mut screen = Screen::new(4, 2);
    screen.put(0, 0, 'T', StyleId::NONE);
    screen
        .write_premeasured(Rect::new(3, 1, 1, 1), &atom("字", 1, None))
        .unwrap();
    let out = Painter::new(None).try_paint(&screen, &[]).unwrap();
    // vt100 0.16 ignores DECAWM (mode 7). Use an independent mode-aware
    // terminal model for wrap at the edge, with a literal physical width
    // different from the imported span. No importer geometry is consulted.
    let mut terminal = WrapTerminal::new();
    terminal.process(&out);
    assert_eq!(terminal.cells[0][0], 'T');
    assert_eq!(terminal.scrolls, 0);
    assert!(terminal.wrap);
    terminal.process("\x1b[1;1HX");
    assert_eq!(terminal.cells[0][0], 'X');
    assert_eq!((terminal.row, terminal.column), (0, 1));
    // The same fixture genuinely exercises wrapping when the guard is gone.
    let mut unguarded = WrapTerminal::new();
    unguarded.process(&out.replace("\x1b[?7l", ""));
    assert!(unguarded.scrolls > 0);
}

#[test]
fn premeasured_gap_replay_cannot_emit_uncertain_atoms() {
    let mut screen = Screen::new(4, 1);
    screen
        .write_premeasured(Rect::new(1, 0, 1, 1), &atom("é", 1, None))
        .unwrap();
    let mut painter = Painter::new(None);
    painter.try_paint(&screen, &[]).unwrap();
    screen.put(0, 0, 'A', StyleId::NONE);
    screen.put(2, 0, 'B', StyleId::NONE);
    let out = painter.try_paint(&screen, &[screen.area()]).unwrap();
    assert!(!out.contains('é'), "{out:?}");
    let ascii = imported("A", 1, 2);
    let out = Painter::new(None).try_paint(&ascii, &[]).unwrap();
    assert!(out.contains("A "), "ASCII remains certain: {out:?}");
}

#[test]
fn premeasured_inline_error_is_before_every_painter_mutation() {
    let native = Screen::new(3, 3);
    let mut painter = Painter::new(None).inline();
    painter.try_paint(&native, &[]).unwrap();
    painter.move_to(2, 2);
    let before_cursor = painter.cursor;
    let before = painter.previous.as_ref().unwrap().cells.clone();
    let before_written = painter.written.clone();
    let before_stats = painter.stats;
    let mut screen = native.clone();
    screen
        .write_premeasured(Rect::new(2, 2, 1, 1), &atom("字", 1, None))
        .unwrap();
    let mut out = String::from("prefix");
    let error = painter
        .try_paint_into(&screen, &[screen.area()], &mut out)
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(out, "prefix");
    assert_eq!(painter.cursor, before_cursor);
    assert_eq!(painter.previous.as_ref().unwrap().cells, before);
    assert_eq!(painter.written, before_written);
    assert_eq!(painter.stats, before_stats);
    assert!(painter.try_repaint(&screen, &[screen.area()]).is_err());
    assert!(!painter.force);
    assert_eq!(painter.move_to(0, 2), "\r");
    assert!(!painter.finish().contains("?7"));
}

#[test]
fn premeasured_clipped_invalid_grid_never_interns_earlier_text_or_styles() {
    let unique = "never intern this invalid premeasured grid atom e7c970c4";
    let style = Style::parse("bold yellow on blue").unwrap();
    let mut screen = Screen::new(1, 1);
    let before = screen.clone();
    let cells = [
        PremeasuredCell {
            text: unique,
            width: 1,
            style: Some(&style),
        },
        PremeasuredCell {
            text: "",
            width: 1,
            style: None,
        },
    ];
    assert!(
        screen
            .write_premeasured(Rect::new(0, 0, 2, 1), &cells)
            .is_err()
    );
    assert!(!graphemes().read().unwrap().index.contains_key(unique));
    assert_eq!(screen.style_count(), before.style_count());
    assert_eq!(screen.cells, before.cells);
}

#[test]
fn premeasured_split_preserves_old_column_styles_and_scroll_crop_is_whole() {
    let mut screen = Screen::new(6, 2);
    screen
        .write_premeasured(Rect::new(1, 0, 3, 1), &atom("atom", 3, None))
        .unwrap();
    screen.restyle(1, 0, &Style::parse("red").unwrap());
    screen.restyle(3, 0, &Style::parse("blue").unwrap());
    let left = screen.cell(1, 0).style;
    let right = screen.cell(3, 0).style;
    let damage = screen
        .write_premeasured(Rect::new(2, 0, 1, 1), &atom("X", 1, None))
        .unwrap();
    assert_eq!(damage, Rect::new(1, 0, 3, 1));
    assert_eq!(
        (screen.cell(1, 0).text(), screen.cell(1, 0).style),
        (" ", left)
    );
    assert_eq!(
        (screen.cell(3, 0).text(), screen.cell(3, 0).style),
        (" ", right)
    );
    screen
        .write_premeasured(Rect::new(1, 1, 3, 1), &atom("atom", 3, None))
        .unwrap();
    screen.scroll_up(Rect::new(3, 0, 2, 2), 1);
    assert!(screen.cells.iter().all(|cell| cell.width == 1));
}

// A deliberately small, independent VT model for this edge fixture. It
// accepts only emitted cursor/erase/SGR/DECAWM controls, fails on any other
// sequence, and assigns 字 a literal physical width two. This supplements
// vt100's CUP tests because that dependency does not implement mode 7.
struct WrapTerminal {
    cells: [[char; 4]; 2],
    row: usize,
    column: usize,
    wrap: bool,
    scrolls: usize,
}

impl WrapTerminal {
    fn new() -> Self {
        Self {
            cells: [[' '; 4]; 2],
            row: 0,
            column: 0,
            wrap: true,
            scrolls: 0,
        }
    }

    fn newline(&mut self) {
        if self.row == 1 {
            self.cells[0] = self.cells[1];
            self.cells[1] = [' '; 4];
            self.scrolls += 1;
        } else {
            self.row += 1;
        }
    }

    fn process(&mut self, text: &str) {
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => {
                    assert_eq!(chars.next(), Some('['));
                    let mut parameters = String::new();
                    let final_byte = loop {
                        let next = chars.next().expect("complete CSI");
                        if ('@'..='~').contains(&next) {
                            break next;
                        }
                        parameters.push(next);
                    };
                    if parameters == "?7" {
                        assert!(matches!(final_byte, 'h' | 'l'));
                        self.wrap = final_byte == 'h';
                        continue;
                    }
                    let numbers: Vec<usize> = parameters
                        .split(';')
                        .map(|p| p.parse().unwrap_or(1))
                        .collect();
                    let n = numbers[0];
                    match final_byte {
                        'H' => {
                            self.row = n.saturating_sub(1).min(1);
                            self.column = numbers
                                .get(1)
                                .copied()
                                .unwrap_or(1)
                                .saturating_sub(1)
                                .min(3);
                        }
                        'A' => self.row = self.row.saturating_sub(n),
                        'B' => self.row = (self.row + n).min(1),
                        'C' => self.column = (self.column + n).min(3),
                        'D' => self.column = self.column.saturating_sub(n),
                        'J' => {
                            assert_eq!(n, 2);
                            self.cells = [[' '; 4]; 2];
                        }
                        'K' => self.cells[self.row][self.column.min(4)..].fill(' '),
                        'X' => self.cells[self.row][self.column.min(4)..(self.column + n).min(4)]
                            .fill(' '),
                        'm' => {}
                        _ => panic!("unsupported CSI {parameters}{final_byte}"),
                    }
                }
                '\r' => self.column = 0,
                '\n' => self.newline(),
                c => {
                    let width = if c == '字' {
                        2
                    } else {
                        assert!(c.is_ascii_graphic() || c == ' ');
                        1
                    };
                    if self.column + width > 4 {
                        if self.wrap {
                            self.column = 0;
                            self.newline();
                        } else {
                            self.column = self.column.min(3);
                        }
                    }
                    self.cells[self.row][self.column] = c;
                    self.column = (self.column + width).min(4);
                }
            }
        }
    }
}

#[test]
fn premeasured_provenance_changes_are_painted_in_both_directions() {
    for strategy in PaintStrategy::ALL {
        let mut screen = Screen::new(1, 1);
        screen.put(0, 0, 'A', StyleId::NONE);
        let mut painter = Painter::new(None);
        painter.set_strategy(strategy);
        painter.try_paint(&screen, &[]).unwrap();
        screen
            .write_premeasured(screen.area(), &atom("A", 1, None))
            .unwrap();
        let out = painter.try_paint(&screen, &[screen.area()]).unwrap();
        assert!(out.contains('A') && out.contains("?7l"));
        assert!(
            painter
                .try_paint(&screen, &[screen.area()])
                .unwrap()
                .is_empty()
        );
        screen.put(0, 0, 'A', StyleId::NONE);
        let out = painter.try_paint(&screen, &[screen.area()]).unwrap();
        assert!(out.contains('A') && !out.contains("?7l"));
        assert_eq!(painter.previous.as_ref().unwrap().cells, screen.cells);
    }
}
