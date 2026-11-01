//! Shapes `#[derive(Inspect)]` and `#[inspect]` accept.
use std::borrow::Cow;
use std::fmt::Debug;

use rich_ext::{Inspect, inspect};

/// Generic with a where clause, a lifetime and a raw identifier.
#[derive(Inspect)]
#[inspect(methods)]
pub struct Borrowed<'a, T, const N: usize>
where
    T: Clone,
{
    pub r#type: Cow<'a, str>,
    pub items: [T; N],
    #[inspect(skip)]
    pub _unused: std::marker::PhantomData<fn() -> T>,
}

// Bare `#[inspect]` next to the derive's `#[inspect(…)]` helpers.
#[inspect]
impl<'a, T: Clone + Debug, const N: usize> Borrowed<'a, T, N> {
    /// The first item.
    pub const fn len(&self) -> usize {
        N
    }

    /// # Safety
    /// None needed.
    pub unsafe extern "C" fn raw(&self) {}

    pub fn generic<U>(&self, value: U) -> U
    where
        U: Into<u8>,
    {
        value
    }
}

#[derive(Inspect)]
pub enum Tree<T: Debug> {
    Leaf(T),
    Branch {
        #[inspect(nested)]
        left: Box<Tree<T>>,
        #[inspect(nested)]
        right: Option<Box<Tree<T>>>,
    },
}

#[derive(Inspect)]
#[doc = "Docs from an attribute."]
pub struct Attr(#[inspect(skip)] pub std::cell::Cell<u8>);

fn main() {
    let value = Borrowed::<u8, 2> {
        r#type: Cow::Borrowed("x"),
        items: [1, 2],
        _unused: std::marker::PhantomData,
    };
    let _ = rich::Console::new().render_to_string(&Inspect::new(&value).all(true));
    let tree = Tree::Branch {
        left: Box::new(Tree::Leaf(1)),
        right: None,
    };
    let _ = rich::Console::new().render_to_string(&Inspect::new(&tree));
    let _ = rich::Console::new().render_to_string(&Inspect::new(&Attr(Default::default())));
}
