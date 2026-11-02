struct Point;

#[rich_ext::inspect]
impl Clone for Point {
    fn clone(&self) -> Self {
        Point
    }
}

#[rich_ext::inspect(extra)]
impl Point {}

fn main() {}
