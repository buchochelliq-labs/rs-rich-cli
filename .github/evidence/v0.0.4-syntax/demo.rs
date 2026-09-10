// Repeated source keeps its syntax colors and layout.
fn main() {
    let message = "café — ready";
    println!("{message}");
    println!("{message}");
    /* The same text inside a comment stays a comment:
    println!("{message}");
    */
    println!("{message}");
}
