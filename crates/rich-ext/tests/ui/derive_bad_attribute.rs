#[derive(rich_ext::Rich)]
struct A {
    #[rich(style = "bodl")]
    a: u8,
    #[rich(colour = "red")]
    b: u8,
}
fn main() {}
