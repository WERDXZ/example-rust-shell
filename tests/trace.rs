// #[cfg(test)]
// mod test {
//
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{env, thread};

const CARGO_DIR: &str = env!("CARGO_MANIFEST_DIR");
// fn tsh() -> String {
//     // env::var("CARGO_BIN_EXE_tsh").unwrap_or("".to_string())
//     env::current_exe()
//         .expect("No exe found")
//         .to_str()
//         .unwrap()
//         .to_string()
// }

fn kill(pid: &str, sig: &str) {
    Command::new("kill")
        .args(["-s", sig, pid])
        .status()
        .expect("Failed to send sigal");
}

fn driver(exec: &str, trace: &str, args: &str) -> String {
    let mut child = Command::new(exec)
        .arg(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(&format!("{exec} not found"));

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    let trace = File::open(trace).expect(&format!("trace file {trace} not found"));

    let reader = BufReader::new(trace);

    for line in reader.lines() {
        let line = line.expect("IO failure");
        let mut line_iter = line.split_whitespace();
        match line_iter.nth(0) {
            None => {
                continue;
            }
            Some(s) => match s {
                "#" => {
                    continue;
                }
                "TSTP" => {
                    kill(&child.id().to_string(), "TSTP");
                }
                "INT" => {
                    kill(&child.id().to_string(), "INT");
                }
                "QUIT" => {
                    kill(&child.id().to_string(), "QUIT");
                }
                "KILL" => {
                    kill(&child.id().to_string(), "KILL");
                }
                "CLOSE" => {
                    // write!(stdin, "\0").unwrap();
                    drop(stdin);
                    assert!((child.wait().unwrap()).success());
                    break;
                }
                "WAIT" => {
                    assert!((child.wait().unwrap()).success());
                    break;
                }
                "SLEEP" => {
                    let time = match line_iter.nth(1) {
                        Some(time) => time.parse::<u64>().unwrap_or(0),
                        None => 0,
                    };
                    thread::sleep(Duration::from_secs(time));
                }
                _ => {
                    writeln!(stdin, "{}", line).expect("unable to write to pipe");
                }
            },
        }
    }
    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .expect("Failed to read stdout");
    output
}

#[test]
fn trace01() {
    let out = (driver(
        &format!("{}/{}", CARGO_DIR, "target/debug/tsh"),
        &format!("{}/tests/{}", CARGO_DIR, "trace01.txt"),
        "p",
    ));
    let refout = (driver(
        &format!("{}/{}", CARGO_DIR, "bin/tshref"),
        &format!("{}/tests/{}", CARGO_DIR, "trace01.txt"),
        "p",
    ));
    similar_asserts::assert_eq!(out, refout);
}

// }
