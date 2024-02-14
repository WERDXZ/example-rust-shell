use std::process::Command;
use std::path::Path;

const FILES: [&str; 4] = ["myint", "mysplit", "mystop", "myspin"];

fn main() {
    for file in FILES {
        build_c(file);
    }
}

fn build_c(name: &str) {
    Command::new("gcc")
        .arg(format!("{}.c", name))
        .args(["-o", name])
        .status()
        .expect("process failed to execute");
}
