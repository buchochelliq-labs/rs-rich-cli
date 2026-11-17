#![cfg(feature = "art")]

mod alpha {
    use intuituive::{App, ImageFit, ImageView, Picture, Pixels, image_graphics, picture_of};
    use rich_art::image::{DynamicImage, Rgba, RgbaImage};
    use rich_record::{
        render::raster::{self, Fonts, Frame},
        screen::Theme,
        terminal::Terminal,
    };

    #[test]
    fn transparent_native_png_over_widget_matches_clean_background() {
        let fonts = Fonts::embedded();
        let theme = Theme {
            background: (17, 35, 61),
            ..Theme::default()
        };
        let frame = Frame {
            title: "",
            key: None,
            size: 16.,
            window: false,
            caption: None,
        };
        for transparent in [false, true] {
            for background in [None, Some([24, 111, 72])] {
                let image = RgbaImage::from_fn(8, 16, |x, y| {
                    if transparent || (x == 0 && y == 0) {
                        Rgba([0, 0, 0, 0])
                    } else if x == 0 {
                        Rgba([200, 20, 10, 100])
                    } else {
                        Rgba([200, 20, 10, 255])
                    }
                });
                let picture = Picture::new(DynamicImage::ImageRgba8(image));
                let mut view = ImageView::new()
                    .fit(ImageFit::Stretch)
                    .pixels(Pixels::Iterm);
                if let Some(bg) = background {
                    view = view.background(bg);
                }
                let copy = picture.clone();
                let app = App::new(move || {
                    picture_of(
                        {
                            let p = copy.clone();
                            move || Some(p.clone())
                        },
                        view,
                    )
                    .fixed(1)
                })
                .graphics(image_graphics());
                let mut driver = app.driver(1, 1);
                driver.set_cell_pixels(Some((8, 16)));
                driver.update(std::time::Duration::ZERO);
                let mut actual = Terminal::new(1, 1);
                let output = driver.render().unwrap();
                actual.process(output.as_bytes()).unwrap();
                let actual = actual.snapshot(&theme);
                assert_eq!(actual.images.len(), 1);
                let rgba = actual.images[0].image.pixels().unwrap();
                assert_eq!(rgba[3], 0, "PNG corner alpha was lost");
                assert_eq!(
                    rgba[8 * 4 + 3],
                    if transparent { 0 } else { 100 },
                    "partial alpha was lost"
                );
                let mut clean = Terminal::new(1, 1);
                clean.process(b"\x1b[?25l").unwrap();
                if let Some([r, g, b]) = background {
                    clean
                        .process(format!("\x1b[48;2;{r};{g};{b}m\x1b[1X").as_bytes())
                        .unwrap();
                }
                clean
                    .process(picture.encode(Pixels::Iterm, 1, 1).unwrap().as_bytes())
                    .unwrap();
                let clean = clean.snapshot(&theme);
                assert_eq!(
                    actual.images[0].image.pixels(),
                    clean.images[0].image.pixels()
                );
                let a = raster::render(&actual, &theme, &fonts, &frame);
                let b = raster::render(&clean, &theme, &fonts, &frame);
                let different = a
                    .pixels
                    .chunks(3)
                    .zip(b.pixels.chunks(3))
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(
                    different, 0,
                    "native alpha exposes the half-block underlay bg={background:?} transparent={transparent} actual={:?} clean={:?}",
                    actual.rows, clean.rows
                );
            }
        }
    }

    #[test]
    fn accepted_overlay_clear_restores_cursor_and_style_without_nested_save_slots() {
        let picture = Picture::new(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            8,
            16,
            Rgba([255, 0, 0, 127]),
        )));
        let console = rich::Console::builder().width(1).build();
        for pixels in [Pixels::Iterm, Pixels::Sixel] {
            let lines = picture.lines(
                &console,
                ImageView::new().pixels(pixels).background([7, 9, 11]),
                1,
                1,
            );
            let frame = rich_ext::frame::Frame::from_segments(&lines[0]);
            let graphics = image_graphics().placements(&frame);
            let mut terminal = Terminal::new(5, 8);
            terminal
                .process(b"\x1b[3;4H\x1b[1;38;2;13;27;49;48;2;3;5;7m")
                .unwrap();
            terminal
                .process(graphics[0].graphic.draw(0).as_bytes())
                .unwrap();
            assert_eq!(terminal.screen().cursor_position(), (2, 3));
            terminal.process(b"Z").unwrap();
            let snapshot = terminal.snapshot(&Theme::default());
            let cell = &snapshot.rows[2][3];
            assert_eq!(cell.text, "Z");
            assert_eq!(cell.fg, (13, 27, 49));
            assert_eq!(cell.bg, (3, 5, 7));
            assert!(cell.bold);
        }
    }
}
mod scenes {
    use intuituive::{App, ImageFit, ImageView, Picture, Pixels, image_graphics, signal};
    use rich::Segment;
    use rich_art::image::{DynamicImage, Rgba, RgbaImage};
    use rich_record::{
        render::raster::{self, Fonts, Frame},
        screen::Theme,
        terminal::Terminal,
    };
    use std::{cell::Cell, rc::Rc, time::Duration};

    // Required Frame methods only: the provided Screen adapter is the reference.
    struct FrameSource(std::sync::Arc<dyn rich_ext::frame::PlacementSource>);
    impl rich_ext::frame::PlacementSource for FrameSource {
        fn placements(&self, frame: &rich_ext::frame::Frame) -> Vec<rich_ext::frame::Placement> {
            self.0.placements(frame)
        }
        fn placements_with_metrics(
            &self,
            frame: &rich_ext::frame::Frame,
            metrics: Option<(u16, u16)>,
        ) -> Vec<rich_ext::frame::Placement> {
            self.0.placements_with_metrics(frame, metrics)
        }
    }

    type SceneState = (bool, usize, bool);
    type SceneHandle = Rc<Cell<Option<intuituive::Signal<SceneState>>>>;

    fn make(
        picture: Picture,
        pixels: Pixels,
        state: SceneState,
        frame_reference: bool,
    ) -> (intuituive::Driver, SceneHandle) {
        let handle = Rc::new(Cell::new(None));
        let copy = handle.clone();
        let app = App::new(move || {
            let state = signal(state);
            copy.set(Some(state));
            let picture = picture.clone();
            intuituive::node::leaf(move |console, _, _| {
                let (cover, scroll, visible) = state.get();
                if !visible {
                    return vec![vec![Segment::new("gone", None)]];
                }
                let mut lines = picture.lines(
                    console,
                    ImageView::new().fit(ImageFit::Stretch).pixels(pixels),
                    8,
                    4,
                );
                if cover {
                    for line in lines.iter_mut().take(3).skip(1) {
                        let right = Segment::divide(line, &[4, 8]).remove(1);
                        *line = vec![Segment::new("DDDD", None)];
                        line.extend(right);
                    }
                }
                lines.into_iter().skip(scroll).collect()
            })
        })
        .graphics(if frame_reference {
            std::sync::Arc::new(FrameSource(image_graphics()))
        } else {
            image_graphics()
        });
        let mut driver = app.driver(8, 4);
        driver.set_cell_pixels(Some((8, 16)));
        (driver, handle)
    }
    fn render(term: &Terminal) -> Vec<u8> {
        let theme = Theme {
            background: (17, 35, 61),
            ..Theme::default()
        };
        raster::render(
            &term.snapshot(&theme),
            &theme,
            &Fonts::embedded(),
            &Frame {
                title: "",
                key: None,
                size: 16.,
                window: false,
                caption: None,
            },
        )
        .pixels
    }
    #[test]
    fn dialog_scroll_resize_dismiss_remove_agree_with_fresh_scene() {
        let picture = Picture::new(DynamicImage::ImageRgba8(RgbaImage::from_fn(
            64,
            64,
            |x, y| Rgba([(x * 3) as u8, (y * 3) as u8, 20, 255]),
        )));
        for pixels in [Pixels::Cells, Pixels::Kitty, Pixels::Sixel, Pixels::Iterm] {
            let (mut driver, state) = make(picture.clone(), pixels, (false, 0, true), false);
            let mut terminal = Terminal::new(4, 8);
            for (step, shown) in [
                (false, 0, true),
                (true, 0, true),
                (true, 1, true),
                (false, 1, true),
                (false, 0, true),
                (false, 0, false),
                (false, 0, true),
            ]
            .into_iter()
            .enumerate()
            {
                if step > 0 {
                    state.get().unwrap().set(shown);
                }
                let (width, height) = if step == 3 { (6, 3) } else { (8, 4) };
                if step == 3 || step == 4 {
                    driver.resize(width, height);
                    terminal.set_size(height, width).unwrap();
                }
                driver.update(Duration::from_millis(step as u64));
                terminal
                    .process(driver.render().unwrap_or_default().as_bytes())
                    .unwrap();
                let (mut fresh, _) = make(picture.clone(), pixels, shown, true);
                fresh.resize(width, height);
                fresh.update(Duration::ZERO);
                let mut expected = Terminal::new(height, width);
                expected
                    .process(fresh.render().unwrap_or_default().as_bytes())
                    .unwrap();
                let a = render(&terminal);
                let b = render(&expected);
                let count = a.iter().zip(&b).filter(|(a, b)| a != b).count();
                assert_eq!(
                    count,
                    0,
                    "{pixels:?} transition {step} actual_tiles={:?} expected_tiles={:?}",
                    terminal
                        .snapshot(&Theme::default())
                        .images
                        .iter()
                        .map(|t| (
                            t.row,
                            t.column,
                            t.rows,
                            t.columns,
                            t.source,
                            t.image.width(),
                            t.image.height()
                        ))
                        .collect::<Vec<_>>(),
                    expected
                        .snapshot(&Theme::default())
                        .images
                        .iter()
                        .map(|t| (
                            t.row,
                            t.column,
                            t.rows,
                            t.columns,
                            t.source,
                            t.image.width(),
                            t.image.height()
                        ))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    #[test]
    fn adjacent_driver_copies_share_upload_and_restore_after_boundary_overlay() {
        let picture = Picture::new(DynamicImage::ImageRgba8(RgbaImage::from_fn(
            32,
            32,
            |x, y| Rgba([(x * 7) as u8, (y * 7) as u8, (x + y) as u8, 255]),
        )));
        for (cols, rows) in [(1u16, 1u16), (4, 2)] {
            for pixels in [Pixels::Kitty, Pixels::Iterm, Pixels::Sixel] {
                let make = |covered| {
                    let handle = Rc::new(Cell::new(None));
                    let copy = handle.clone();
                    let image = picture.clone();
                    let app = App::new(move || {
                        let cover = signal(covered);
                        copy.set(Some(cover));
                        let image = image.clone();
                        intuituive::node::leaf(move |console, _, _| {
                            let lines = image.lines(
                                console,
                                ImageView::new()
                                    .fit(ImageFit::Stretch)
                                    .pixels(pixels)
                                    .background([17, 35, 61]),
                                cols,
                                rows,
                            );
                            let mut repeated: Vec<Vec<Segment>> = lines
                                .iter()
                                .map(|line| (0..3).flat_map(|_| line.clone()).collect())
                                .collect();
                            if cover.get() {
                                let parts = Segment::divide(
                                    &repeated[0],
                                    &[
                                        usize::from(cols - 1),
                                        usize::from(cols + 1),
                                        usize::from(cols * 3),
                                    ],
                                );
                                repeated[0] = parts[0].clone();
                                repeated[0].push(Segment::new("DD", None));
                                repeated[0].extend(parts[2].clone());
                            }
                            repeated
                        })
                    })
                    .graphics(image_graphics());
                    let mut driver = app.driver(cols * 3, rows);
                    driver.set_cell_pixels(Some((8, 16)));
                    (driver, handle)
                };
                let (mut driver, cover) = make(false);
                let mut terminal = Terminal::new(rows, cols * 3);
                for (step, covered) in [false, true, false].into_iter().enumerate() {
                    if step > 0 {
                        cover.get().unwrap().set(covered);
                    }
                    driver.update(Duration::ZERO);
                    let output = driver.render().unwrap_or_default();

                    if pixels == Pixels::Kitty {
                        assert_eq!(
                            output.matches("a=T,").count(),
                            usize::from(step == 0),
                            "one shared initial upload, retained through overlay: {cols}x{rows} step={step}"
                        );
                    }
                    if pixels != Pixels::Kitty && step == 0 {
                        let marker = if pixels == Pixels::Iterm {
                            "]1337;"
                        } else {
                            "\x1bP"
                        };
                        assert_eq!(
                            output.matches(marker).count(),
                            3,
                            "each adjacent overlay is drawn"
                        );
                    }
                    terminal.process(output.as_bytes()).unwrap();
                    let snapshot = terminal.snapshot(&Theme::default());
                    // Cells some image shows in: an opaque picture drawn
                    // again over cells left as they were lies over the last.
                    let shown: std::collections::HashSet<(u16, u16)> = snapshot
                        .images
                        .iter()
                        .flat_map(|tile| {
                            (tile.row..tile.row + tile.rows).flat_map(move |row| {
                                (tile.column..tile.column + tile.columns)
                                    .map(move |column| (row, column))
                            })
                        })
                        .collect();
                    assert_eq!(
                        shown.len(),
                        usize::from(cols * 3 * rows) - if covered { 2 } else { 0 },
                        "visible {pixels:?} {cols}x{rows} step={step}"
                    );
                    if covered {
                        assert_eq!(snapshot.rows[0][usize::from(cols - 1)].text, "D");
                        assert_eq!(snapshot.rows[0][usize::from(cols)].text, "D");
                    }
                    let (mut fresh, _) = make(covered);
                    fresh.update(Duration::ZERO);
                    let mut expected = Terminal::new(rows, cols * 3);
                    expected
                        .process(fresh.render().unwrap_or_default().as_bytes())
                        .unwrap();
                    assert!(
                        render(&terminal) == render(&expected),
                        "{pixels:?} {cols}x{rows} step={step} differs from fresh scene"
                    );
                }
            }
        }
    }
}
mod scroll {
    //! #884: a page of pictures scrolled through a view, in the overlay
    //! protocols: an unchanged picture is encoded once for each part of it
    //! shown, and the terminal ends each step as a fresh draw of it would.
    use intuituive::node::{column, label, scroll_with};
    use intuituive::{App, Driver, ImageFit, ImageView, Picture, Pixels, image_graphics};
    use rich_art::image::{DynamicImage, Rgb, RgbImage};
    use rich_art::picture::picture_encodes;
    use rich_record::{
        render::raster::{self, Fonts, Frame},
        screen::Theme,
        terminal::Terminal,
    };
    use std::{cell::Cell, rc::Rc, time::Duration};

    const WIDTH: u16 = 8;
    const HEIGHT: u16 = 6;
    /// Each picture's first row on the page: three rows each, a label above.
    const TOPS: [u16; 3] = [1, 5, 9];

    fn pictures() -> Vec<Picture> {
        (0..3u8)
            .map(|k| {
                Picture::new(DynamicImage::ImageRgb8(RgbImage::from_fn(
                    32,
                    24,
                    |x, y| Rgb([(x * 8) as u8, (y * 10) as u8, 80 * k]),
                )))
            })
            .collect()
    }

    fn page(
        pictures: &[Picture],
        pixels: Pixels,
        at: u16,
    ) -> (Driver, Rc<Cell<Option<intuituive::Signal<u16>>>>) {
        let handle = Rc::new(Cell::new(None));
        let copy = handle.clone();
        let pictures = pictures.to_vec();
        let app = App::new(move || {
            let offset = intuituive::signal(at);
            copy.set(Some(offset));
            let view = ImageView::new().fit(ImageFit::Stretch).pixels(pixels);
            let mut nodes = Vec::new();
            for (k, picture) in pictures.iter().enumerate() {
                nodes.push(label(format!("text {k}")).fixed(1));
                let picture = picture.clone();
                nodes.push(intuituive::picture_of(move || Some(picture.clone()), view).fixed(3));
            }
            nodes.push(label("end").fixed(1));
            scroll_with(column(nodes), offset)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(WIDTH, HEIGHT);
        driver.set_cell_pixels(Some((8, 16)));
        (driver, handle)
    }

    fn raster(term: &Terminal) -> Vec<u8> {
        let theme = Theme::default();
        raster::render(
            &term.snapshot(&theme),
            &theme,
            &Fonts::embedded(),
            &Frame {
                title: "",
                key: None,
                size: 16.,
                window: false,
                caption: None,
            },
        )
        .pixels
    }

    /// Offsets down the page and back.
    fn steps() -> Vec<u16> {
        (0..=7).chain((0..7).rev()).collect()
    }

    #[test]
    fn scrolling_encodes_an_unchanged_picture_once_for_each_part_shown() {
        for pixels in [Pixels::Sixel, Pixels::Iterm] {
            let pictures = pictures();
            let (mut driver, offset) = page(&pictures, pixels, 0);
            driver.update(Duration::ZERO);
            let _ = driver.render();
            let mut seen = std::collections::HashSet::new();
            let mut last = 0;
            for at in steps().into_iter().skip(1) {
                let before: Vec<u64> = pictures.iter().map(|p| picture_encodes(p.id())).collect();
                offset.get().unwrap().set(at);
                driver.update(Duration::ZERO);
                let _ = driver.render();
                for (k, picture) in pictures.iter().enumerate() {
                    let encodes = picture_encodes(picture.id()) - before[k];
                    // The rows of it in view, before and now.
                    let part = |at: u16| {
                        let top = TOPS[k].max(at);
                        let bottom = (TOPS[k] + 3).min(at + HEIGHT);
                        (top < bottom).then_some((top - TOPS[k], bottom - top))
                    };
                    let (was, now) = (part(last), part(at));
                    if was == now || now.is_none() || seen.contains(&(k, now)) {
                        assert_eq!(
                            encodes, 0,
                            "{pixels:?} {last}->{at}: picture {k} encoded again"
                        );
                    } else {
                        assert!(
                            encodes <= 1,
                            "{pixels:?} {last}->{at}: picture {k} {encodes}"
                        );
                    }
                    seen.insert((k, now));
                }
                last = at;
            }
        }
    }

    #[test]
    fn scrolling_a_page_of_pictures_leaves_no_stale_cells() {
        for pixels in [Pixels::Cells, Pixels::Kitty, Pixels::Sixel, Pixels::Iterm] {
            let pictures = pictures();
            let (mut driver, offset) = page(&pictures, pixels, 0);
            let mut terminal = Terminal::new(HEIGHT, WIDTH);
            for (step, at) in steps().into_iter().enumerate() {
                offset.get().unwrap().set(at);
                driver.update(Duration::from_millis(step as u64));
                terminal
                    .process(driver.render().unwrap_or_default().as_bytes())
                    .unwrap();
                let (mut fresh, _) = page(&pictures, pixels, at);
                fresh.update(Duration::ZERO);
                let mut expected = Terminal::new(HEIGHT, WIDTH);
                expected
                    .process(fresh.render().unwrap_or_default().as_bytes())
                    .unwrap();
                let (a, b) = (raster(&terminal), raster(&expected));
                let differ = a.chunks(3).zip(b.chunks(3)).filter(|(a, b)| a != b).count();
                assert_eq!(
                    differ,
                    0,
                    "{pixels:?} step {step} (offset {at}): {:?} against {:?}",
                    terminal
                        .snapshot(&Theme::default())
                        .images
                        .iter()
                        .map(|t| (t.row, t.column, t.rows, t.columns))
                        .collect::<Vec<_>>(),
                    expected
                        .snapshot(&Theme::default())
                        .images
                        .iter()
                        .map(|t| (t.row, t.column, t.rows, t.columns))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }
}
