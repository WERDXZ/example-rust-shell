use nix::sys::{
    signal::{sigaction, SaFlags, SigAction, SigHandler, Signal},
    signalfd::SigSet,
};
use regex::Regex;
use std::{os::raw::c_int, process::exit};

pub extern "C" fn sigquit_handler(_sigquit: i32) {
    println!("Terminating after receipt of SIGQUIT signal");
    exit(0)
}

pub fn unix_error(msg: &str) -> ! {
    println!("{}: {}", msg, dbg!(nix::errno::Errno::last()));
    exit(1)
}

#[allow(unused)]
pub fn app_error(msg: &str) -> ! {
    println!("{}", msg);
    exit(1)
}

pub fn usage() {
    println!("Usage: shell [-hvp]");
    println!("\t-h   print this message");
    println!("\t-v   print additional diagnostic information");
    println!("\t-p   do not emit a command prompt")
}

pub fn parse_line(line: &str) -> (Vec<String>, bool) {
    let re = Regex::new(r#""([^"]*)"|'([^']*)'|\S+"#).unwrap();
    let mut argv: Vec<String> = re
        .captures_iter(line)
        .filter_map(|cap| {
            cap.get(1)
                .or(cap.get(2))
                .or(cap.get(0))
                .map(|m| m.as_str().to_string())
        })
        .collect();

    let background = if let Some(last) = argv.last() {
        last == "&"
    } else {
        false
    };

    if background {
        argv.pop();
    }

    (argv, background)
}

pub unsafe fn set_handler(
    sig: Signal,
    handler: extern "C" fn(_: c_int),
) -> Result<SigAction, nix::errno::Errno> {
    let mut mask = SigSet::empty();
    mask.add(sig);
    let action = SigAction::new(SigHandler::Handler(handler), SaFlags::SA_RESTART, mask);
    sigaction(sig, &action)
}
