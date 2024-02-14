use std::path::Path;
fn main() {
    println!("{}",Path::new(env!("CARGO_MANIFEST_DIR")).to_str().unwrap());
}
