use rich_ext::Inspect;

#[derive(Inspect)]
union Bits {
    int: u32,
    float: f32,
}

fn main() {}
