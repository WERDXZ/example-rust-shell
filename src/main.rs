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
    os::fd::{AsRawFd, OwnedFd},
    process::exit,
    sync::{LazyLock, Mutex},
};

use i32 as sig_t;

const PROMT_STR: &str = "tsh> ";

static VERBOSE: LazyLock<bool> =
    LazyLock::new(|| args().any(|arg| arg == "-v" || arg == "--verbose"));
static PROMT: LazyLock<bool> = LazyLock::new(|| args().all(|arg| arg != "-p" && arg != "--prompt"));

static LOCK: LazyLock<(OwnedFd, OwnedFd)> = LazyLock::new(|| match pipe() {
    Ok(lock) => lock,
    Err(_e) => unix_error("Create pipe failed"),
});
static JOBMANAGER: LazyLock<Mutex<JobManager>> = LazyLock::new(|| Mutex::new(JobManager::new()));

fn main() {
    if *VERBOSE {
        println!("tsh: Version 1.0");
    }

    if args().any(|arg| arg == "-h" || arg == "--help") {
        helpers::usage();
        return;
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
}

fn start() {
    loop {
        {}
        let mut line = String::new();
        if *PROMT {
            print!("{}", PROMT_STR);
            match stdout().flush() {
                Ok(_) => {}
                Err(e) => unix_error(&dbg!(e).to_string()),
            };
        }

        match stdin().read_line(&mut line) {
            Ok(_) => {}
            Err(e) => {
                unix_error(&dbg!(e).to_string());
            }
        }
        if line.is_empty() {
            break;
        }
        eval(&line);
    }
}

fn eval(line: &str) {
    let (argv, isbg) = helpers::parse_line(line);

    if argv.is_empty() {
        return;
    }

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
                    if let Ok(job) = { JOBMANAGER.lock().unwrap().get_jid(jid) } {
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
                    if let Ok(job) = { JOBMANAGER.lock().unwrap().get_jid(jid) } {
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
        .clone()
        .into_iter()
        .map(|s| CString::new(s.clone()).unwrap())
        .collect();
    let mut argvc: Vec<&CStr> = vec![];
    for arg in argvt.iter() {
        argvc.push(arg.as_c_str());
    }
    let res = match unsafe { fork() } {
        Ok(res) => res,
        Err(_e) => unix_error("Cannot fork"),
    };

    match res {
        ForkResult::Parent { child } => {
            if isbg {
                let next_jid = {
                    let mut manager = JOBMANAGER.lock().unwrap();
                    manager
                        .add_job(Job::new(child, States::BG, line.to_string()))
                        .unwrap();
                    manager.next_jid()
                };
                sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
                println!("[{}] ({}) {}", next_jid, child.as_raw(), line.trim_end());
            } else {
                {
                    let mut manager = JOBMANAGER.lock().unwrap();
                    manager
                        .add_job(Job::new(child, States::FG, line.to_string()))
                        .unwrap();
                }
                sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
                waitfg(child);
            }
        }
        ForkResult::Child => {
            setpgid(Pid::from_raw(0), Pid::from_raw(0)).unwrap();
            match execv(argcc, argvc.as_slice()) {
                Ok(_) => {}
                Err(e) => {
                    if e == Errno::ENOENT {
                        println!("{}: Command not found", argv[0]);
                        exit(0);
                    }
                    unix_error("Execv Error");
                }
            };
        }
    }
}

fn quit() {
    exit(0);
}

fn jobs() {
    println!("{}", JOBMANAGER.lock().unwrap().list().trim_end());
}

fn bg(target: Pid) {
    let mut manager = JOBMANAGER.lock().unwrap();
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
    let target = {
        let mut manager = JOBMANAGER.lock().unwrap();
        match manager.set_state(target, States::FG) {
            Ok(job) => job.pid,
            Err(_e) => {
                println!("({}): Not found", target.as_raw());
                return;
            }
        }
    };

    sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
    jobs();
    waitfg(target);
}

fn waitfg(_pid: Pid) {
    let mut buffer: [u8; 4] = [0, 0, 0, 0];
    match read(LOCK.0.as_raw_fd(), &mut buffer) {
        Ok(_) => {}
        Err(_e) => unix_error("Pipe Read Error"),
    };
}

extern "C" fn sigstp_handler(sigquit: sig_t) {
    let fgpid = match JOBMANAGER.lock().unwrap().fg() {
        Some(pid) => pid.as_raw(),
        None => return,
    };

    kill(Pid::from_raw(-fgpid), Signal::try_from(sigquit).unwrap()).unwrap();
}

extern "C" fn sigint_handler(sigquit: sig_t) {
    let fgpid = match JOBMANAGER.lock().unwrap().fg() {
        Some(pid) => pid.as_raw(),
        None => return,
    };

    kill(Pid::from_raw(-fgpid), Signal::try_from(sigquit).unwrap()).unwrap();
}

extern "C" fn sigchld_handler(_sigchld: sig_t) {
    let mut flag = WaitPidFlag::empty();
    let fgpid = match JOBMANAGER.lock().unwrap().fg() {
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
                JOBMANAGER
                    .lock()
                    .unwrap()
                    .set_state(pid, States::ST)
                    .unwrap();
                println!(
                    "Job [{}] ({}) stopped by signal {}",
                    { JOBMANAGER.lock().unwrap().get_pid(pid).unwrap().jid },
                    pid,
                    signal as i32
                );
                match fgpid {
                    -1 => {}
                    _ => match write(&LOCK.1, &[0, 0, 0, 0]) {
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
                    { JOBMANAGER.lock().unwrap().get_pid(pid).unwrap().jid },
                    pid,
                    signal as i32
                );
                JOBMANAGER.lock().unwrap().remove_job(pid).unwrap();
                match fgpid {
                    -1 => {}
                    _ => match write(&LOCK.1, &[0, 0, 0, 0]) {
                        Ok(_) => {}
                        Err(_e) => {
                            unix_error("Pipe write failed");
                        }
                    },
                }
            }
            WaitStatus::Exited(pid, _exitcode) => {
                JOBMANAGER.lock().unwrap().remove_job(pid).unwrap();
                match fgpid {
                    -1 => {}
                    _ => match write(&LOCK.1, &[0, 0, 0, 0]) {
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
