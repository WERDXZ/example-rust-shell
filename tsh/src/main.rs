mod helpers;
mod jobs;

use crate::jobs::Job;
use helpers::unix_error;
use jobs::{JobManager, Jobs, States};
use nix::{
    errno::Errno,
    sys::{
        signal::{kill, sigprocmask, SigmaskHow, Signal},
        signalfd::SigSet,
        wait::{waitpid, WaitPidFlag, WaitStatus},
    },
    unistd::{execv, fork, pipe, read, setpgid, write, ForkResult, Pid},
};
use std::{
    env::args,
    ffi::{CStr, CString},
    io::{stdin, stdout, Write},
    process::exit,
    ptr,
    sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering},
};

use i32 as sig_t;

const PROMT_STR: &'static str = "tsh>";

static VERBOSE: AtomicBool = AtomicBool::new(false);
static PROMT: AtomicBool = AtomicBool::new(true);

static LOCK: (AtomicI32, AtomicI32) = (AtomicI32::new(-1), AtomicI32::new(-1));
static JOBMANAGER: AtomicPtr<JobManager> = AtomicPtr::new(ptr::null_mut());

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
    }

    unsafe {
        init();
    }

    start();
}

unsafe fn init() {
    use helpers::{set_handler, sigquit_handler};

    match set_handler(Signal::SIGQUIT, sigquit_handler) {
        Ok(_) => {}
        Err(_e) => unix_error("Set SIGQUIT handler failed"),
    }
    match set_handler(Signal::SIGTSTP, sigstp_handler) {
        Ok(_) => {}
        Err(_e) => unix_error("Set SIGSTOP handler failed"),
    }
    match set_handler(Signal::SIGINT, sigint_handler) {
        Ok(_) => {}
        Err(_e) => unix_error("Set SIGINT handler failed"),
    }
    match set_handler(Signal::SIGCHLD, sigchld_handler) {
        Ok(_) => {}
        Err(_e) => unix_error("Set SIGCHLD handler failed"),
    }

    let lock = match pipe() {
        Ok(lock) => lock,
        Err(_e) => unix_error("Create pipe failed"),
    };

    LOCK.0.store(lock.0, Ordering::Relaxed);
    LOCK.1.store(lock.1, Ordering::Relaxed);

    JOBMANAGER.store( Box::leak(Box::new(JobManager::new())), Ordering::Release);
}

fn start() {
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
        if line.len() == 0 {
            break;
        }
        eval(&line);
    }
}

fn eval(line: &str) {
    let (argv, isbg) = helpers::parse_line(line);
    if isbg {
        println!("get background job");
    }

    if argv.len() == 0 {
        return;
    }

    let manager = unsafe { JOBMANAGER.load(Ordering::Relaxed).as_mut().unwrap() };

    match argv[0].as_str() {
        "quit" => quit(),
        "jobs" => jobs(),
        "bg" => {
            if argv.len() == 1 {
                println!("bg command requires PID or %jobid argument");
                return;
            }
            if let Ok(pid) = argv[1].parse::<i32>() {
                // bg(&mut manager.get_pid_mut(Pid::from_raw(pid)));
                bg(Pid::from_raw(pid));
            } else if argv[1].chars().next().unwrap_or('\0') == '%' {
                if let Ok(jid) = argv[1][1..].parse::<u32>() {
                    if let Ok(job) = manager.get_jid(jid) {
                        bg(job.pid);
                    }
                }
            }
        }
        "fg" => {
            if argv.len() == 1 {
                println!("fg command requires PID or %jobid argument");
                return;
            }
            if let Ok(pid) = argv[1].parse::<i32>() {
                fg(Pid::from_raw(pid));
            } else if argv[1].chars().next().unwrap_or('\0') == '%' {
                if let Ok(jid) = argv[1][1..].parse::<u32>() {
                    if let Ok(job) = manager.get_jid(jid) {
                        fg(job.pid);
                    }
                }
            }
        }
        _ => {
            exec(line, argv, isbg);
        }
    };
}

fn exec(line: &str, argv: Vec<String>, isbg: bool) {
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
        let manager = JOBMANAGER.load(Ordering::Relaxed).as_mut().unwrap();

        match res {
            ForkResult::Parent { child } => {
                if isbg {
                    let next_jid = manager.next_jid();
                    manager
                        .add_job(Job::new(child, States::BG, line.to_string()))
                        .unwrap();
                    sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
                    println!("[{}] ({}) {}", next_jid, child.as_raw(), line.trim_end());
                } else {
                    manager
                        .add_job(Job::new(child, States::FG, line.to_string()))
                        .unwrap();
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

fn jobs() {
    println!(
        "{}",
        unsafe { JOBMANAGER.load(Ordering::Relaxed).as_ref().unwrap() }
            .list()
            .trim_end()
    );
}

fn bg(target: Pid) {
    let manager = unsafe { JOBMANAGER.load(Ordering::Relaxed).as_mut().unwrap() };
    match kill(target, Signal::SIGCONT) {
        Ok(_) => {}
        Err(_) => unix_error("Send SIGCONT failed"),
    };
    let target = match manager.set_state(target, States::BG) {
        Ok(job) => job,
        Err(_e) => {
            println!("({}): Not found", target.as_raw());
            return;
        }
    };
    println!("[{}] ({}) {}", target.jid, target.pid, target.cmd);
}

fn fg(target: Pid) {
    let manager = unsafe { JOBMANAGER.load(Ordering::Relaxed).as_mut().unwrap() };
    let mut mask: SigSet = SigSet::empty();
    mask.add(Signal::SIGCHLD);
    match sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None) {
        Ok(_) => {}
        Err(_e) => unix_error("Unable to block signal"),
    };
    match kill(target, Signal::SIGCONT) {
        Ok(_) => {}
        Err(_) => unix_error("Send SIGCONT failed"),
    };
    let target = match manager.set_state(target, States::FG) {
        Ok(job) => job,
        Err(_e) => {
            println!("({}): Not found", target.as_raw());
            return;
        }
    };

    sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
    jobs();
    waitfg(target.pid);
}

fn waitfg(_pid: Pid) {
    let mut buffer: [u8; 4] = [0, 0, 0, 0];
    match read(LOCK.0.load(Ordering::Relaxed), &mut buffer) {
        Ok(_) => {}
        Err(_e) => unix_error("Pipe Read Error"),
    };
}

extern "C" fn sigstp_handler(sigquit: sig_t) {
    let manager = unsafe { JOBMANAGER.load(Ordering::Relaxed).as_mut().unwrap() };
    let fgpid = match manager.fg() {
        Some(pid) => pid.as_raw(),
        None => return,
    };

    kill(Pid::from_raw(-fgpid), Signal::try_from(sigquit).unwrap()).unwrap();
}

extern "C" fn sigint_handler(sigquit: sig_t) {
    let manager = unsafe { JOBMANAGER.load(Ordering::Relaxed).as_mut().unwrap() };
    let fgpid = match manager.fg() {
        Some(pid) => pid.as_raw(),
        None => return,
    };

    kill(Pid::from_raw(-fgpid), Signal::try_from(sigquit).unwrap()).unwrap();
}

extern "C" fn sigchld_handler(_sigchld: sig_t) {
    let manager = unsafe { JOBMANAGER.load(Ordering::Relaxed).as_mut().unwrap() };
    let mut flag = WaitPidFlag::empty();
    let fgpid = match manager.fg() {
        Some(pid) => pid.as_raw(),
        None => -1,
    };
    flag.set(WaitPidFlag::WNOHANG, true);
    flag.set(WaitPidFlag::WUNTRACED, true);
    loop {
        let res = match waitpid(None, Some(flag)) {
            Ok(res) => res,
            Err(_e) => {
                if let Errno::ECHILD = _e {
                    break;
                }
                unix_error("WaitPid Error")
            }
        };

        match res {
            WaitStatus::Stopped(pid, signal) => {
                manager.set_state(pid, States::ST).unwrap();
                println!(
                    "Job [{}] ({}) stopped by signal {}",
                    manager.get_pid(pid).unwrap().jid,
                    pid,
                    signal
                );
                match fgpid {
                    -1 => {}
                    _ => match write(LOCK.1.load(Ordering::Relaxed), &[0, 0, 0, 0]) {
                        Ok(_) => {}
                        Err(_e) => {
                            unix_error("Pipe write failed");
                        }
                    },
                }
            }
            WaitStatus::Signaled(pid, signal, _core_dumped) => {
                println!(
                    "Job [{}] ({}) terminated by signal {}",
                    manager.get_pid(pid).unwrap().jid,
                    pid,
                    signal
                );
                manager.remove_job(pid).unwrap();
                match fgpid {
                    -1 => {}
                    _ => match write(LOCK.1.load(Ordering::Relaxed), &[0, 0, 0, 0]) {
                        Ok(_) => {}
                        Err(_e) => {
                            unix_error("Pipe write failed");
                        }
                    },
                }
            }
            WaitStatus::Exited(pid, _exitcode) => {
                manager.remove_job(pid).unwrap();
                match fgpid {
                    -1 => {}
                    _ => match write(LOCK.1.load(Ordering::Relaxed), &[0, 0, 0, 0]) {
                        Ok(_) => {}
                        Err(_e) => {
                            unix_error("Pipe write failed");
                        }
                    },
                }
            }
            WaitStatus::StillAlive => {
                break;
            }
            _ => {}
        }
    }
}
