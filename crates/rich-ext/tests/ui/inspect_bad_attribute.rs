use rich_ext::Inspect;

#[derive(Inspect)]
struct Point {
    #[inspect(rename = "y")]
    pub x: i32,
}

#[derive(Inspect)]
struct Both {
    #[inspect(skip, nested)]
    pub x: i32,
}

#[derive(Inspect)]
#[inspect(title = "no")]
struct Titled;

fn main() {}
