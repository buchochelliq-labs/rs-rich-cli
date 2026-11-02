use rich_ext::Inspect;

struct Opaque;

#[derive(Inspect)]
struct Holder {
    pub id: u8,
    pub handle: Opaque,
}

fn main() {}
