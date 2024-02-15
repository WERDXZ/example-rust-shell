use regex::Regex;
use std::process::exit;

use nix::errno::errno;
pub use nix::sys::signal::signal;

pub extern "C" fn sigquit_handler(sigquit: nix::libc::c_int) {
    println!("Terminating after receipt of SIGQUIT signal");
    exit(0)
}

pub fn unix_error(msg: &str) -> !{
    println!("{}: {}", msg, nix::errno::Errno::last());
    exit(1)
}

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

pub fn parse_line(line: &str) -> (Vec<String>,bool) {
    let re = Regex::new(r#"(\"[^\"]*\"|\'[^\']*\'|\S+)"#).unwrap();
    let argv:Vec<String> = re.captures_iter(line)
        .map(|cap| cap[0].to_string())
        .collect();

    if argv.last().unwrap().chars().next().unwrap() == '&' {
        (argv[0..argv.len()-2].to_vec(),true)
    }else {
        (argv, false)
    }

}

