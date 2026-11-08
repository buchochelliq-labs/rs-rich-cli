#![cfg(feature = "art")]

use intuituive::{ImageFit, ImageView, Picture, Pixels, image_graphics};
use rich::Segment;
use rich_art::image::{DynamicImage, Rgba, RgbaImage};
use rich_ext::frame::{Frame, graphics::plan};

fn picture() -> Picture {
    Picture::new(DynamicImage::ImageRgba8(RgbaImage::from_fn(
        64,
        64,
        |x, y| Rgba([(x * 3) as u8, (y * 3) as u8, (x + y) as u8, 255]),
    )))
}
fn scene(picture: &Picture, pixels: Pixels, hide: impl Fn(usize, usize) -> bool) -> Frame {
    let console = rich::Console::builder()
        .width(8)
        .color_system(Some(rich::color::ColorSystem::Truecolor))
        .build();
    let mut segments = Vec::new();
    for (y, line) in picture
        .lines(
            &console,
            ImageView::new().fit(ImageFit::Stretch).pixels(pixels),
            8,
            4,
        )
        .iter()
        .enumerate()
    {
        for (x, cell) in Segment::divide(line, &(1..=8).collect::<Vec<_>>())
            .into_iter()
            .enumerate()
        {
            if hide(x, y) {
                segments.push(Segment::new("D", None));
            } else {
                segments.extend(cell);
            }
        }
        segments.push(Segment::line());
    }
    Frame::from_segments(&segments)
}

#[test]
fn public_picture_partial_cover_keeps_every_surviving_rectangle() {
    let picture = picture();
    for pixels in [Pixels::Iterm, Pixels::Sixel, Pixels::Kitty] {
        for side in [false, true] {
            let hidden = |x, y| y == 1 && if side { x < 4 } else { x >= 4 };
            let frame = scene(&picture, pixels, hidden);
            let placements = image_graphics().placements(&frame);
            let mut cells = [[false; 8]; 4];
            for p in placements {
                for (y, row) in cells.iter_mut().enumerate().skip(p.row).take(p.rows) {
                    for (x, covered) in row.iter_mut().enumerate().skip(p.column).take(p.cols) {
                        assert!(!hidden(x, y), "graphic covers dialog at {x},{y}");
                        *covered = true;
                    }
                }
            }
            for (y, row) in cells.iter().enumerate() {
                for (x, covered) in row.iter().enumerate() {
                    assert_eq!(*covered, !hidden(x, y), "missing {pixels:?} at {x},{y}");
                }
            }
        }
    }
}

#[test]
fn public_kitty_movement_retains_uploaded_data() {
    let picture = picture();
    let source = image_graphics();
    let previous = source.placements(&scene(&picture, Pixels::Kitty, |_, _| false));
    let mut moved = previous.clone();
    moved[0].row += 2;
    assert!(!previous[0].same(&moved[0]));
    assert!(plan(&previous, &moved, &[]).draw.is_empty());
    assert_eq!(
        plan(&[], &[moved[0].clone(), previous[0].clone()], &[])
            .draw
            .len(),
        1
    );
    assert_eq!(plan(&[], &moved, &[]).draw.len(), 1);
}

#[test]
fn holes_metrics_offsets_and_cells_have_safe_distinct_placements() {
    let picture = picture();
    let source = image_graphics();
    for pixels in [Pixels::Cells, Pixels::Sixel, Pixels::Iterm, Pixels::Kitty] {
        let frame = scene(&picture, pixels, |x, y| {
            (2..6).contains(&x) && (1..3).contains(&y)
        });
        let mut previous_key = None;
        for metrics in [(8, 16), (10, 20), (12, 24)] {
            let placements = source.placements_with_metrics(&frame, Some(metrics));
            if pixels == Pixels::Cells {
                assert!(placements.is_empty());
                continue;
            }
            assert_eq!(placements.len(), 4);
            assert_eq!(
                placements.iter().map(|p| p.cols * p.rows).sum::<usize>(),
                24
            );
            let keys: Vec<_> = placements.iter().map(|p| p.graphic.key()).collect();
            assert_ne!(previous_key.as_ref(), Some(&keys));
            assert_eq!(
                keys,
                source
                    .placements_with_metrics(&frame, Some(metrics))
                    .iter()
                    .map(|p| p.graphic.key())
                    .collect::<Vec<_>>()
            );
            if pixels == Pixels::Kitty {
                assert!(keys.iter().all(|k| *k == keys[0]));
            } else {
                assert_ne!(keys[1], keys[2]);
            }
            previous_key = Some(keys);
        }
    }
}

#[test]
fn checkerboard_fragment_budget_preserves_cells_without_native_overlay() {
    let picture = picture();
    let console = rich::Console::builder().width(32).build();
    let lines = picture.lines(
        &console,
        ImageView::new()
            .fit(ImageFit::Stretch)
            .pixels(Pixels::Iterm),
        32,
        8,
    );
    let mut segments = Vec::new();
    for (y, line) in lines.iter().enumerate() {
        for (x, cell) in Segment::divide(line, &(1..=32).collect::<Vec<_>>())
            .into_iter()
            .enumerate()
        {
            if (x + y) % 2 == 0 {
                segments.extend(cell);
            } else {
                segments.push(Segment::new("D", None));
            }
        }
        segments.push(Segment::line());
    }
    let frame = Frame::from_segments(&segments);
    assert!(image_graphics().placements(&frame).is_empty());
    assert_eq!(frame.height(), 8);
}

#[test]
fn kitty_competing_fits_reupload_when_returning_to_one_view() {
    let picture = picture();
    let source = image_graphics();
    let small = source.placements(&scene(&picture, Pixels::Kitty, |_, _| false));
    let console = rich::Console::builder()
        .width(4)
        .color_system(Some(rich::color::ColorSystem::Truecolor))
        .build();
    let mut lines = Vec::new();
    for line in picture.lines(
        &console,
        ImageView::new()
            .fit(ImageFit::Stretch)
            .pixels(Pixels::Kitty),
        4,
        2,
    ) {
        lines.extend(line);
        lines.push(Segment::line());
    }
    let other = source.placements(&Frame::from_segments(&lines));
    assert_ne!(small[0].graphic.key(), other[0].graphic.key());
    let id = format!("i={},", picture.id());
    assert!(small[0].graphic.draw(0).contains(&id));
    assert!(other[0].graphic.draw(0).contains(&id));
    let mut both = Vec::new();
    for (cols, rows) in [(8, 4), (4, 2)] {
        for line in picture.lines(
            &console,
            ImageView::new()
                .fit(ImageFit::Stretch)
                .pixels(Pixels::Kitty),
            cols,
            rows,
        ) {
            both.extend(line);
            both.push(Segment::line());
        }
    }
    let previous = source.placements(&Frame::from_segments(&both));
    assert_eq!(previous.len(), 2);
    assert_eq!(plan(&previous, &previous, &[]).draw.len(), 2);
    assert_eq!(
        plan(&previous, &small, &[]).draw.len(),
        1,
        "the second fit replaced the retained first upload"
    );
}

#[test]
fn legacy_tags_only_accept_whole_width_and_missing_pictures_fall_back() {
    use rich_art::picture::PictureTag;
    let picture = picture();
    let mut tag = PictureTag {
        id: picture.id(),
        version: picture.version(),
        cols: 8,
        rows: 2,
        row: 0,
        pixels: Pixels::Iterm,
        fit: Some(ImageFit::Stretch),
        anchor: intuituive::ImageAnchor::Center,
    };
    let source = image_graphics();
    let whole = Frame::from_segments(&[Segment::new("abcdefgh", Some(tag.style()))]);
    assert_eq!(source.placements(&whole).len(), 1);
    let part = Frame::from_segments(&[Segment::new("abcd", Some(tag.style()))]);
    assert!(source.placements(&part).is_empty());
    tag.version = u32::MAX;
    assert!(
        source
            .placements(&Frame::from_segments(&[Segment::new(
                "abcdefgh",
                Some(tag.style())
            )]))
            .is_empty()
    );
    tag.version = picture.version();
    tag.row = usize::MAX;
    assert!(
        source
            .placements(&Frame::from_segments(&[Segment::new(
                "abcdefgh",
                Some(tag.style())
            )]))
            .is_empty()
    );
}

#[test]
fn background_provenance_changes_native_graphic_identity() {
    let picture = picture();
    let console = rich::Console::builder().width(8).build();
    let source = image_graphics();
    let placed = |background| {
        let lines = picture.lines(
            &console,
            ImageView::new()
                .fit(ImageFit::Stretch)
                .pixels(Pixels::Iterm)
                .background(background),
            8,
            1,
        );
        source.placements(&Frame::from_segments(&lines[0]))
    };
    let first = placed([1, 2, 3]);
    let other = placed([4, 5, 6]);
    assert_ne!(first[0].graphic.key(), other[0].graphic.key());
    assert!(first[0].graphic.draw(0).contains("48;2;1;2;3m"));
}

#[test]
fn adjacent_same_view_instances_survive_screen_line_coalescing() {
    use rich_ext::surface::{Rect, Screen};
    let picture = picture();
    let console = rich::Console::builder()
        .width(12)
        .color_system(Some(rich::color::ColorSystem::Truecolor))
        .build();
    for (cols, rows) in [(1, 1), (4, 2)] {
        for pixels in [Pixels::Kitty, Pixels::Iterm, Pixels::Sixel] {
            let source = image_graphics();
            let lines = picture.lines(
                &console,
                ImageView::new().fit(ImageFit::Stretch).pixels(pixels),
                cols,
                rows,
            );
            let repeated: Vec<_> = lines
                .iter()
                .map(|line| (0..3).flat_map(|_| line.clone()).collect())
                .collect();
            let mut screen = Screen::new(cols * 3, rows);
            screen.write_lines(screen.area(), &repeated);
            let from_screen = |screen: &Screen| {
                let mut segments = Vec::new();
                for line in screen.lines() {
                    segments.extend(line);
                    segments.push(Segment::line());
                }
                Frame::from_segments(&segments)
            };
            let frame = from_screen(&screen);

            if pixels == Pixels::Kitty {
                assert_eq!(
                    frame.row(0).iter().filter(|run| run.cells() > 0).count(),
                    1,
                    "exercise the actual coalesced run"
                );
                assert_eq!(frame.row(0)[0].cells(), usize::from(cols) * 3);
            }
            let placements = source.placements(&frame);
            assert_eq!(placements.len(), 3, "{pixels:?} {cols}x{rows}");
            for (index, p) in placements.iter().enumerate() {
                assert_eq!(
                    (p.column, p.cols, p.rows),
                    (
                        index * usize::from(cols),
                        usize::from(cols),
                        usize::from(rows)
                    )
                );
                assert_eq!(p.graphic.key(), placements[0].graphic.key());
            }
            let initial = plan(&[], &placements, &[]);
            assert_eq!(
                initial.draw.len(),
                if pixels == Pixels::Kitty { 1 } else { 3 }
            );

            if pixels == Pixels::Kitty {
                assert_eq!(
                    initial
                        .draw
                        .iter()
                        .map(|p| p.graphic.draw(0).matches("a=T,").count())
                        .sum::<usize>(),
                    1
                );
            }
            // Cover a copy boundary. The surviving suffix and following copy
            // again share a coalesced Kitty run, with a source-column restart.
            screen.write_lines(
                Rect::new(cols - 1, 0, 2, 1),
                &[vec![Segment::new("DD", None)]],
            );
            let covered = source.placements(&from_screen(&screen));
            assert_eq!(
                covered.iter().map(|p| p.cols * p.rows).sum::<usize>(),
                usize::from(cols * 3 * rows) - 2
            );
            for p in &covered {
                if p.row == 0 {
                    assert!(
                        p.column + p.cols <= usize::from(cols - 1)
                            || p.column >= usize::from(cols + 1)
                    );
                }
            }

            if pixels == Pixels::Kitty {
                assert!(plan(&placements, &covered, &[]).draw.is_empty());
            }
        }
    }
}

#[test]
fn coalesced_overlay_copies_respect_frame_fragment_budget() {
    use rich_ext::surface::Screen;
    let picture = picture();
    let console = rich::Console::builder()
        .width(129)
        .color_system(Some(rich::color::ColorSystem::Truecolor))
        .build();
    for pixels in [Pixels::Iterm, Pixels::Sixel] {
        let line = picture
            .lines(
                &console,
                ImageView::new().fit(ImageFit::Stretch).pixels(pixels),
                1,
                1,
            )
            .remove(0);
        for count in [128u16, 129] {
            let mut screen = Screen::new(count, 1);
            screen.write_lines(
                screen.area(),
                &[(0..count).flat_map(|_| line.clone()).collect()],
            );
            let frame = Frame::from_segments(&screen.lines()[0]);
            let placements = image_graphics().placements(&frame);
            assert_eq!(
                placements.len(),
                if count == 128 { 128 } else { 0 },
                "{pixels:?} count={count}"
            );
            assert_eq!(frame.row(0)[0].cells(), usize::from(count));
        }
    }
}

#[test]
fn malformed_kitty_provenance_never_becomes_a_whole_width_fallback() {
    use rich_art::kitty::{DIACRITICS, PLACEHOLDER};
    let picture = picture();
    let console = rich::Console::builder()
        .width(4)
        .color_system(Some(rich::color::ColorSystem::Truecolor))
        .build();
    let lines = picture.lines(
        &console,
        ImageView::new()
            .fit(ImageFit::Stretch)
            .pixels(Pixels::Kitty),
        4,
        1,
    );
    let style = lines[0][0].style.clone();
    let cell =
        |row: usize, col: usize| format!("{PLACEHOLDER}{}{}", DIACRITICS[row], DIACRITICS[col]);
    let valid = (0..4).map(|col| cell(0, col)).collect::<String>();
    let malformed = [
        [0, 1, 3, 0]
            .into_iter()
            .map(|col| cell(0, col))
            .collect::<String>(),
        [0, 1, 2, 4]
            .into_iter()
            .map(|col| cell(0, col))
            .collect::<String>(),
        format!("{}{}{}{}", cell(0, 0), cell(1, 1), cell(0, 2), cell(0, 3)),
        format!(
            "{}{}{}{PLACEHOLDER}{}",
            cell(0, 0),
            cell(0, 1),
            cell(0, 2),
            DIACRITICS[0]
        ),
        "    ".to_owned(),
    ];
    let source = image_graphics();
    assert_eq!(
        source
            .placements(&Frame::from_segments(&[Segment::new(valid, style.clone())]))
            .len(),
        1
    );
    for text in malformed {
        let frame = Frame::from_segments(&[Segment::new(text, style.clone())]);
        assert!(
            source.placements(&frame).is_empty(),
            "malformed run must not acquire an image"
        );
    }
}

#[test]
fn driver_four_moves_count_actual_protocol_work() {
    use rich_art::picture::picture_bytes;
    use std::{cell::Cell, rc::Rc, time::Duration};
    for pixels in [Pixels::Cells, Pixels::Kitty, Pixels::Sixel, Pixels::Iterm] {
        let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
        let id = picture.id();
        let handle = Rc::new(Cell::new(None));
        let copy = handle.clone();
        let app = intuituive::App::new(move || {
            let offset = intuituive::signal(0usize);
            copy.set(Some(offset));
            let picture = picture.clone();
            intuituive::node::leaf(move |console, _, _| {
                let mut lines = vec![vec![]; offset.get()];
                lines.extend(picture.lines(
                    console,
                    ImageView::new().fit(ImageFit::Stretch).pixels(pixels),
                    4,
                    3,
                ));
                lines
            })
        })
        .graphics(image_graphics());
        let mut driver = app.driver(4, 5);
        driver.set_cell_pixels(Some((8, 16)));
        driver.update(Duration::ZERO);
        let initial = driver.render().unwrap_or_default();
        let before = picture_bytes(id);
        let mut draws = 0;
        for offset in [1, 2, 1, 0] {
            handle.get().unwrap().set(offset);
            driver.update(Duration::ZERO);
            let output = driver.render().unwrap_or_default();
            draws += match pixels {
                Pixels::Kitty => output.matches("a=T,").count(),
                Pixels::Sixel => output.matches("\x1bP").count(),
                Pixels::Iterm => output.matches("]1337;").count(),
                Pixels::Cells => 0,
            };
        }
        let bytes = picture_bytes(id) - before;
        println!(
            "movement {pixels:?}: initial_output={} graphics_bytes={bytes} draws={draws}",
            initial.len()
        );
        if pixels == Pixels::Kitty || pixels == Pixels::Cells {
            assert_eq!((bytes, draws), (0, 0));
        } else {
            assert!(bytes > 0);
            assert_eq!(draws, 4);
        }
    }
}

#[test]
fn frame_fragment_overflow_keeps_kitty_uploads_and_overlay_fallbacks() {
    let picture = picture();
    let source = image_graphics();
    let console = rich::Console::builder()
        .width(1)
        .color_system(Some(rich::color::ColorSystem::Truecolor))
        .build();
    let view = ImageView::new().fit(ImageFit::Stretch);
    let overlay = picture.lines(&console, view.pixels(Pixels::Iterm), 1, 1);
    let kitty = picture.lines(&console, view.pixels(Pixels::Kitty), 1, 1);
    let mut segments = Vec::new();
    for _ in 0..129 {
        segments.extend(overlay[0].clone());
    }
    segments.extend(kitty[0].clone());
    let frame = Frame::from_segments(&segments);
    let placements = source.placements(&frame);
    assert_eq!(placements.len(), 1);
    assert!(placements[0].graphic.in_cells());
    assert_eq!(placements[0].column, 129);
}

#[test]
fn releasing_kitty_data_invalidates_previous_retention_proof() {
    let picture = picture();
    let source = image_graphics();
    let frame = scene(&picture, Pixels::Kitty, |_, _| false);
    let previous = source.placements(&frame);
    assert!(source.release(&[]).contains("a=d"));
    let next = source.placements(&frame);
    assert_eq!(
        plan(&previous, &next, &[]).draw.len(),
        1,
        "released data must be uploaded again even with old placements held"
    );
}
