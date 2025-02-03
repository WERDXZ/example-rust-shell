use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{env, thread};

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use regex::Regex;

const CARGO_DIR: &str = env!("CARGO_MANIFEST_DIR");
const PID1: &str = r"\(\d+\)";
const PID2: &str = r"$\d+\s";

fn driver(exec: &str, trace: &str, args: &str) -> String {
    env::set_current_dir(Path::new(&format!("{}/{}", CARGO_DIR, "bin"))).unwrap();
    let mut child = Command::new(exec)
        .arg(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("{exec} not found"));

    let mut stdin_wrapper = child.stdin.take();
    let mut stdout = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    let trace = File::open(trace).unwrap_or_else(|_| panic!("trace file {trace} not found"));

    let reader = BufReader::new(trace);

    for line in reader.lines() {
        let line = line.expect("IO failure");
        let mut line_iter = line.split_whitespace();
        match line_iter.next() {
            None => {
                continue;
            }
            Some(s) => match dbg!(s) {
                "#" => {
                    continue;
                }
                "TSTP" => {
                    kill(Pid::from_raw(child.id() as i32), Signal::SIGTSTP).unwrap();
                    child.try_wait().unwrap();
                }
                "INT" => {
                    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).unwrap();
                    child.try_wait().unwrap();
                }
                "QUIT" => {
                    kill(Pid::from_raw(child.id() as i32), Signal::SIGQUIT).unwrap();
                    child.try_wait().unwrap();
                }
                "KILL" => {
                    kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL).unwrap();
                    child.try_wait().unwrap();
                }
                "CLOSE" => {
                    let stdin = stdin_wrapper.take().unwrap();
                    drop(stdin);
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
                _ => if let Some(ref mut stdin) = stdin_wrapper {
                    writeln!(stdin, "{}", line).expect("unable to write to pipe");
                },
            },
        }
    }
    //assert!((
    dbg!(child.wait()).unwrap();
    //).success());
    // (child.wait().unwrap());
    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .expect("Failed to read stdout");
    dbg!(output.to_string());
    let reg1 = Regex::new(PID1).unwrap();
    let reg2 = Regex::new(PID2).unwrap();
    let output = reg1.replace_all(&output, "");
    let output = reg2.replace_all(&output, "");
    dbg!(output.to_string())
}

macro_rules! test {
    ($name:ident) => {
        #[test]
        fn $name() {
            let refout = driver(
                &format!("{}/{}", CARGO_DIR, "/bin/tshref"),
                &format!("{}/tests/{}.txt", CARGO_DIR, stringify!($name)),
                "-p",
            );
            let out = driver(
                &format!("{}/{}", CARGO_DIR, "target/debug/tsh"),
                &format!("{}/tests/{}.txt", CARGO_DIR, stringify!($name)),
                "-p",
            );
            similar_asserts::assert_eq!(out, refout);
        }
    };
}

test!(trace01);
test!(trace02);
test!(trace03);
test!(trace04);
test!(trace05);
test!(trace06);
test!(trace07);
test!(trace08);
test!(trace09);
test!(trace10);
test!(trace11);
test!(trace12);
test!(trace13);
test!(trace14);
test!(trace15);
test!(trace16);
