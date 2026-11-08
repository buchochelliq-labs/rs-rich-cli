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

    type SceneState = (bool, usize, bool);
    type SceneHandle = Rc<Cell<Option<intuituive::Signal<SceneState>>>>;

    fn make(
        picture: Picture,
        pixels: Pixels,
        state: SceneState,
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
        .graphics(image_graphics());
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
            let (mut driver, state) = make(picture.clone(), pixels, (false, 0, true));
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
                let (mut fresh, _) = make(picture.clone(), pixels, shown);
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
}
