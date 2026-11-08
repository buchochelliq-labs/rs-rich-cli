#![cfg(feature = "art")]

use intuituive::{ImageFit, ImageView, Picture, Pixels};
use rich_art::image::{DynamicImage, Rgba, RgbaImage};
use rich_art::picture::PictureTag;
use rich_ext::qa::profile::{AllocStats, CountingAllocator};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::system();

#[test]
fn native_provenance_keeps_exact_cells_with_bounded_construction_allocations() {
    let console = rich::Console::builder().width(80).build();
    let picture = Picture::new(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        160,
        96,
        Rgba([31, 72, 113, 255]),
    )));
    let view = ImageView::new()
        .fit(ImageFit::Stretch)
        .background([11, 22, 33]);
    // Warm shared rendering caches before comparing the same public operation.
    drop(picture.lines(&console, view, 80, 24));
    let before = AllocStats::now().unwrap();
    let plain = picture.lines(&console, view, 80, 24);
    let plain_allocations = AllocStats::now().unwrap().since(&before).allocations;
    let before = AllocStats::now().unwrap();
    let tagged = picture.lines(&console, view.pixels(Pixels::Iterm), 80, 24);
    let tagged_cost = AllocStats::now().unwrap().since(&before);
    let tagged_allocations = tagged_cost.allocations;
    eprintln!("provenance allocations: plain={plain_allocations}, tagged={tagged_allocations}");
    let cells = 80 * 24;
    // Provenance owns its metadata; constructing intermediate styles must not
    // multiply those per-cell allocations. This leaves room for row overhead.
    assert!(
        tagged_allocations <= plain_allocations + cells * 6 + 256,
        "plain={plain_allocations}, tagged={tagged_allocations}, cells={cells}"
    );
    assert_eq!(plain.len(), 24);
    assert_eq!(tagged.len(), plain.len());
    for (row, (plain, tagged)) in plain.iter().zip(&tagged).enumerate() {
        assert_eq!(plain.len(), 80);
        assert_eq!(plain.len(), tagged.len());
        for (column, (plain, tagged)) in plain.iter().zip(tagged).enumerate() {
            assert_eq!(plain.text, tagged.text);
            let style = tagged.style.as_ref().unwrap();
            assert_eq!(PictureTag::column_of(style), Some(column));
            let tag = PictureTag::of(style).unwrap();
            assert_eq!(
                (tag.id, tag.cols, tag.rows, tag.row),
                (picture.id(), 80, 24, row)
            );
            assert_eq!(PictureTag::background_of(style), Some([11, 22, 33]));
            let expected = plain.style.clone().unwrap_or_default().combine(
                &tag.style_at(column).combine(&rich::Style::from_meta(
                    [(
                        "intuituive.image.background",
                        rich::style::MetaValue::List(
                            [11, 22, 33].map(rich::style::MetaValue::Int).to_vec(),
                        ),
                    )]
                    .into_iter()
                    .collect(),
                )),
            );
            assert_eq!(*style, expected);
        }
    }
    let before = AllocStats::now().unwrap();
    let compact = picture
        .bound(false)
        .lines(&console, view.pixels(Pixels::Iterm), 80, 24);
    let compact_cost = AllocStats::now().unwrap().since(&before);
    eprintln!("bound compact cost: legacy={tagged_cost:?}, compact={compact_cost:?}");
    assert!(compact_cost.allocations < tagged_cost.allocations);
    assert!(compact_cost.live_bytes < tagged_cost.live_bytes);
    for (row, line) in compact.iter().enumerate() {
        for (column, cell) in line.iter().enumerate() {
            let style = cell.style.as_ref().unwrap();
            assert_eq!(PictureTag::column_of(style), Some(column));
            assert_eq!(PictureTag::of(style).unwrap().row, row);
            assert_eq!(PictureTag::binding_of(style), Some(false));
            assert_eq!(PictureTag::background_of(style), Some([11, 22, 33]));
            assert_eq!(cell.text, tagged[row][column].text);
        }
    }
}
