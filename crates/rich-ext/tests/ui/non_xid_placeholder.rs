fn main() {
    let _ = rich_ext::richf!("{a²}");
    let x = 1;
    let _ = rich_ext::richf!("{x:w²$}", x);
}
