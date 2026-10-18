use rich::Console;
use rich_data::profile::{Profile, ProfileOptions};

#[test]
fn scratch() {
    let text = include_str!("fixtures/requests.jsonl");
    let p = Profile::from_json_lines(text.as_bytes(), ProfileOptions::default()).unwrap();
    for width in [80usize, 90] {
        let console = Console::builder().width(width).color_system(None).build();
        let out = console.render_export(&p.to_table());
        for line in out.lines().take(4) {
            println!("{} {}", rich::cells::cell_len(line), line);
        }
    }
}
