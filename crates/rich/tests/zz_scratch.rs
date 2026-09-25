use rich::*;
#[test]
fn scratch() {
    let c = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(10)
        .highlight(false)
        .no_color(false)
        .build();
    let p = Padding::new(Box::new(Text::new("a\n")), (0, 1, 0, 1))
        .style(Style::parse("on red").unwrap());
    println!("{:?}", c.capture(|c| c.print(&p)));
    println!(
        "{:?}",
        c.capture(|c| c.print(&Panel::new(Box::new(Text::new("a\n\n")))))
    );
    println!("{:?}", c.capture(|c| c.print(&Text::new("a\n"))));
    println!("{:?}", Text::new("a\n").rich_render(&c, &c.options()));
}
