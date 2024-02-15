mod helpers;
mod jobs;

use crate::jobs::Job;
use helpers::unix_error;
use jobs::{JobManager, Jobs, States};
use nix::{
    errno::Errno,
    libc::c_int as sig_t,
    sys::{
        signal::{kill, signal, sigprocmask, SigHandler, SigmaskHow, Signal},
        signalfd::SigSet,
    },
    unistd::{execv, fork, pipe, read, setpgid, ForkResult, Pid},
};
use std::{
    env::args,
    ffi::{CStr, CString},
    io::{stdin, stdout, Write},
    process::exit,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

const PROMT_STR: &'static str = "tsh>";

static VERBOSE: AtomicBool = AtomicBool::new(false);
static PROMT: AtomicBool = AtomicBool::new(true);

static LOCK: (AtomicI32, AtomicI32) = (AtomicI32::new(-1), AtomicI32::new(-1));

fn main() {
    for arg in args() {
        if arg == "-v" || arg == "--verbose" {
            VERBOSE.store(true, Ordering::Relaxed);
            continue;
        }
        if arg == "-p" || arg == "--promt" {
            PROMT.store(false, Ordering::Relaxed);
            continue;
        }
        if arg == "-h" || arg == "--help" {
            helpers::usage();
            continue;
        }
        helpers::usage();
    }

    unsafe {
        init();
    }

    start();
}

unsafe fn init() {
    signal(
        Signal::SIGQUIT,
        SigHandler::Handler(helpers::sigquit_handler),
    )
    .unwrap();
    signal(Signal::SIGSTOP, SigHandler::Handler(sigstop_handler)).unwrap();
    signal(Signal::SIGINT, SigHandler::Handler(sigint_handler)).unwrap();
    signal(Signal::SIGCHLD, SigHandler::Handler(sigchld_handler)).unwrap();

    let lock = match pipe() {
        Ok(lock) => lock,
        Err(_e) => unix_error("Create pipe failed"),
    };

    LOCK.0.store(lock.0, Ordering::Relaxed);
    LOCK.1.store(lock.1, Ordering::Relaxed);
}

fn start() {
    let mut job_manager = JobManager::new();
    loop {
        {}
        let mut line = String::new();
        if PROMT.load(Ordering::Relaxed) {
            print!("{}", PROMT_STR);
            match stdout().flush() {
                Ok(_) => {}
                Err(e) => unix_error(&e.to_string()),
            };
        }

        match stdin().read_line(&mut line) {
            Ok(_) => {}
            Err(e) => {
                unix_error(&e.to_string());
            }
        }
        eval(&line, &mut job_manager);
    }
}

fn eval<JobM: Jobs>(line: &str, manager: &mut JobM) {
    let (argv, isbg) = helpers::parse_line(line);

    match argv[0].as_str() {
        "quit" => quit(),
        "jobs" => jobs(manager),
        "bg" => {
            if let Ok(pid) = argv[1].parse::<i32>() {
                // bg(&mut manager.get_pid_mut(Pid::from_raw(pid)));
                if let Ok(job) = manager.get_pid_mut(Pid::from_raw(pid)) {
                    bg(job);
                }
            } else if argv[1].chars().next().unwrap_or('\0') == '%' {
                if let Ok(jid) = argv[1][1..].parse::<u32>() {
                    if let Ok(job) = manager.get_jid_mut(jid) {
                        bg(job);
                    }
                }
            }
        }
        "fg" => {
            if let Ok(pid) = argv[1].parse::<i32>() {
                // bg(&mut manager.get_pid_mut(Pid::from_raw(pid)));
                if let Ok(job) = manager.get_pid_mut(Pid::from_raw(pid)) {
                    fg(job);
                }
            } else if argv[1].chars().next().unwrap_or('\0') == '%' {
                if let Ok(jid) = argv[1][1..].parse::<u32>() {
                    if let Ok(job) = manager.get_jid_mut(jid) {
                        fg(job);
                    }
                }
            }
        }
        _ => {
            exec(manager, line, argv, isbg);
        }
    };
}

fn exec(manager: &mut dyn Jobs, line: &str, argv: Vec<String>, isbg: bool) {
    let mut mask: SigSet = SigSet::empty();
    mask.add(Signal::SIGCHLD);
    match sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None) {
        Ok(_) => {}
        Err(_e) => unix_error("Unable to block signal"),
    };
    // prepare argc and argv
    let argct = CString::new(argv[0].clone()).unwrap();
    let argcc = argct.as_c_str();
    let argvt: Vec<CString> = argv
        .into_iter()
        .map(|s| CString::new(s.clone()).unwrap())
        .collect();
    let mut argvc: Vec<&CStr> = vec![];
    for arg in argvt.iter() {
        argvc.push(arg.as_c_str());
    }
    unsafe {
        let res = match fork() {
            Ok(res) => res,
            Err(_e) => unix_error("Cannot fork"),
        };

        match res {
            ForkResult::Parent { child } => {
                if isbg {
                    let next_jid = manager.next_jid();
                    manager.add_job(Job {
                        pid: child,
                        jid: next_jid,
                        state: States::BG,
                        cmd: line.to_string(),
                    }).unwrap();
                    sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
                    println!("[{}] ({}) {}", next_jid, child.as_raw(), line.trim_end());
                }
                else {
                    let next_jid = manager.next_jid();
                    manager.add_job(Job {
                        pid: child,
                        jid: next_jid,
                        state: States::FG,
                        cmd: line.to_string(),
                    }).unwrap();
                    sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
                    waitfg(child);
                }
            }
            ForkResult::Child => {
                setpgid(Pid::from_raw(0), Pid::from_raw(0)).unwrap();
                execv(argcc, argvc.as_slice()).unwrap();
                #[allow(unreachable_code)]
                {
                    if Errno::last() == Errno::ENOENT {
                        println!("{}: Command not found", argv[0]);
                    } else {
                        unix_error("Execv Error");
                    }
                }
            }
        }
    }
}

fn quit() {
    exit(0);
}

fn jobs(manager: &dyn Jobs) {
    println!("{}", manager.list().trim_end());
}

fn bg(target: &mut Job) {
    target.state = States::BG;
    match kill(target.pid, Signal::SIGCONT) {
        Ok(_) => {}
        Err(_) => unix_error("Send SIGCONT failed"),
    };
    println!("[{}] ({}) {}", target.jid, target.pid, target.cmd);
}

fn fg(target: &mut Job) {
    target.state = States::FG;
    match kill(target.pid, Signal::SIGCONT) {
        Ok(_) => {}
        Err(_) => unix_error("Send SIGCONT failed"),
    };
    waitfg(target.pid);
}

fn waitfg(_pid: Pid) {
    let mut buffer: [u8; 4] = [0, 0, 0, 0];
    match read(LOCK.0.load(Ordering::Relaxed), &mut buffer) {
        Ok(_) => {}
        Err(_e) => unix_error("Pipe Read Error"),
    };
}

extern "C" fn sigstop_handler(sigquit: sig_t) {
    unimplemented!()
}

extern "C" fn sigint_handler(sigquit: sig_t) {
    unimplemented!()
}

extern "C" fn sigchld_handler(sigquit: sig_t) {
    unimplemented!()
}
