use rich::*;
fn main() {
    let c = Console::builder().width(16).force_terminal(true).color_system(Some(ColorSystem::Truecolor)).highlight(false).build();
    let out = |r: &dyn Renderable| println!("{:?}", c.render_export(r).split('\n').nth(1).unwrap().to_string());
    for j in [Justify::Left, Justify::Center, Justify::Right, Justify::Full] {
        let mk = |m: &str, base: Option<&str>| { let mut t = Text::from_markup(m).unwrap(); if let Some(b) = base { t.set_base_style(Style::parse(b).unwrap()); } t.justify(j) };
        for t in [mk("abc", Some("red")), mk("[red]abc[/]", None), mk("x[red]abc[/]", Some("bold"))] {
            out(&Panel::new(Box::new(t)));
        }
    }
    let mut tb = Table::new(); tb.add_column("hhhhhhh"); tb.add_row_cells(vec![Cell::Text(Text::from_markup("[red]a[/][red]b[/]").unwrap())]);
    println!("{:?}", c.render_export(&tb).split('\n').nth(3).unwrap());
}
