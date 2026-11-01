use rich_ext::Inspect;

#[derive(Inspect)]
#[inspect(methods)]
struct Point {
    pub x: i32,
}

fn main() {}
