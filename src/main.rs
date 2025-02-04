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
    unistd::{execv, fork, setpgid, ForkResult, Pid},
};
use std::{
    env::args,
    ffi::{CStr, CString},
    io::{stdin, stdout, Write},
    process::exit,
    sync::{
        mpsc,
        mpsc::{Receiver, Sender},
        LazyLock, Mutex,
    },
};

use i32 as sig_t;
#[derive(Debug)]
enum MessageQueue {
    RemoveJob { pid: Pid },
    Stopped { pid: Pid, signal: i32 },
    Signaled { pid: Pid, signal: i32 },
    Signal { signal: i32 },
}

const PROMT_STR: &str = "tsh> ";

static VERBOSE: LazyLock<bool> =
    LazyLock::new(|| args().any(|arg| arg == "-v" || arg == "--verbose"));
static PROMT: LazyLock<bool> = LazyLock::new(|| args().all(|arg| arg != "-p" && arg != "--prompt"));

type Key = Mutex<Sender<()>>;
type Lock = Mutex<Receiver<()>>;
static LOCK: LazyLock<(Key, Lock)> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel::<()>();
    (Mutex::new(tx), Mutex::new(rx))
});
static JOBMANAGER: LazyLock<Mutex<JobManager>> = LazyLock::new(|| Mutex::new(JobManager::new()));

type SenderT = Mutex<Sender<MessageQueue>>;
type ReceiverT = Mutex<Receiver<MessageQueue>>;
static MESSAGES: LazyLock<(SenderT, ReceiverT)> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel::<MessageQueue>();
    (Mutex::new(tx), Mutex::new(rx))
});

macro_rules! log {
    ($($arg:tt)*) => {
        if *VERBOSE {
            println!($($arg)*);
        }
    };
}

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
    std::thread::spawn(receiver);
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
        "quit" => exit(0),
        "jobs" => println!("{}", JOBMANAGER.lock().unwrap().list().trim_end()),
        "bg" => {
            if argv.len() == 1 {
                println!("bg command requires PID or %jobid argument");
                return;
            }
            if let Ok(pid) = argv[1].parse::<i32>() {
                if let Ok(job) = { JOBMANAGER.lock().unwrap().get_pid_mut(Pid::from_raw(pid)) } {
                    job.bg();
                }
            } else if argv[1].chars().next().unwrap_or('\0') == '%' {
                if let Ok(jid) = argv[1][1..].parse::<u32>() {
                    if let Ok(job) = { JOBMANAGER.lock().unwrap().get_jid_mut(jid) } {
                        job.bg();
                    }
                }
            }
        }
        "fg" => {
            if argv.len() == 1 {
                println!("fg command requires PID or %jobid argument");
                return;
            }
            if let Some(pid) = if let Ok(pid) = argv[1].parse::<i32>() {
                if let Ok(job) = JOBMANAGER.lock().unwrap().get_pid_mut(Pid::from_raw(pid)) {
                    job.fg();
                    Some(job.pid)
                } else {
                    None
                }
            } else if argv[1].chars().next().unwrap_or('\0') == '%' {
                if let Ok(jid) = argv[1][1..].parse::<u32>() {
                    if let Ok(job) = JOBMANAGER.lock().unwrap().get_jid_mut(jid) {
                        job.fg();
                        Some(job.pid)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            } {
                JOBMANAGER.lock().unwrap().set_fg(pid);
                println!("{}", JOBMANAGER.lock().unwrap().list().trim_end());
                waitfg();
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
                let jid = {
                    let mut manager = JOBMANAGER.lock().unwrap();
                    manager
                        .add_job(Job::new(child, States::BG, line.to_string()))
                        .unwrap()
                };
                sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
                println!("[{}] ({}) {}", jid, child.as_raw(), line.trim_end());
            } else {
                {
                    let mut manager = JOBMANAGER.lock().unwrap();
                    manager
                        .add_job(Job::new(child, States::FG, line.to_string()))
                        .unwrap();
                }
                sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None).unwrap();
                waitfg();
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

impl Job {
    fn bg(&mut self) {
        match kill(self.pid, Signal::SIGCONT) {
            Ok(_) => {}
            Err(_) => unix_error("Send SIGCONT failed"),
        };
        self.state = States::BG;

        log!("Backgrounding job id: {}", self.jid);

        println!("[{}] ({}) {}", self.jid, self.pid, self.cmd);
    }

    fn fg(&mut self) {
        let mut mask: SigSet = SigSet::empty();
        mask.add(Signal::SIGCHLD);
        match sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None) {
            Ok(_) => {}
            Err(_e) => unix_error("Unable to block signal"),
        };

        match kill(self.pid, Signal::SIGCONT) {
            Ok(_) => {}
            Err(_) => unix_error("Send SIGCONT failed"),
        };

        self.state = States::FG;

        match sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&mask), None) {
            Ok(_) => {}
            Err(_e) => unix_error("Unable to unblock signal"),
        };

        log!("Forgrounding job id: {}", self.jid);
    }
}

fn waitfg() {
    LOCK.1.lock().unwrap().recv().unwrap();
}

extern "C" fn sigstp_handler(sigstp: sig_t) {
    log!("Received SIGTSTP");
    MESSAGES
        .0
        .lock()
        .unwrap()
        .send(MessageQueue::Signal { signal: sigstp })
        .unwrap();
}

extern "C" fn sigint_handler(sigquit: sig_t) {
    log!("Received SIGINT");
    MESSAGES
        .0
        .lock()
        .unwrap()
        .send(MessageQueue::Signal { signal: sigquit })
        .unwrap();
}

extern "C" fn sigchld_handler(_sigchld: sig_t) {
    log!("Received SIGCHLD");
    let mut flag = WaitPidFlag::empty();
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

        log!("Waitpid returned: {:?}", res);

        match res {
            WaitStatus::Stopped(pid, signal) => {
                log!("Handling stopped job");
                MESSAGES
                    .0
                    .lock()
                    .unwrap()
                    .send(MessageQueue::Stopped {
                        pid,
                        signal: signal as i32,
                    })
                    .unwrap();
            }
            WaitStatus::Signaled(pid, signal, _core_dumped) => {
                log!("Handling signaled job");
                MESSAGES
                    .0
                    .lock()
                    .unwrap()
                    .send(MessageQueue::Signaled {
                        pid,
                        signal: signal as i32,
                    })
                    .unwrap();
            }
            WaitStatus::Exited(pid, _exitcode) => {
                log!("Handling exited job");
                MESSAGES
                    .0
                    .lock()
                    .unwrap()
                    .send(MessageQueue::RemoveJob { pid })
                    .unwrap();
            }
            WaitStatus::StillAlive => {
                break;
            }
            _ => {}
        }

        log!("Message sent");
    }
}

fn receiver() {
    log!("Receiver started");
    loop {
        let message = MESSAGES.1.lock().unwrap().recv().unwrap();
        log!("Message received: {:?}", message);
        let fgpid = match JOBMANAGER.lock().unwrap().current() {
            Some(pid) => pid.as_raw(),
            None => -1,
        };
        log!("Current FG: {}", fgpid);
        match message {
            MessageQueue::RemoveJob { pid } => {
                JOBMANAGER.lock().unwrap().remove_job(pid).unwrap();
                match fgpid {
                    -1 => {}
                    _ => LOCK.0.lock().unwrap().send(()).unwrap(),
                }
            }
            MessageQueue::Stopped { pid, signal } => {
                JOBMANAGER
                    .lock()
                    .unwrap()
                    .set_state(pid, States::ST)
                    .unwrap();
                println!(
                    "Job [{}] ({}) stopped by signal {}",
                    { JOBMANAGER.lock().unwrap().get_pid(pid).unwrap().jid },
                    pid,
                    signal
                );
                match fgpid {
                    -1 => {}
                    _ => LOCK.0.lock().unwrap().send(()).unwrap(),
                }
            }
            MessageQueue::Signaled { pid, signal } => {
                JOBMANAGER.lock().unwrap().remove_job(pid).unwrap();

                println!(
                    "Job [{}] ({}) terminated by signal {}",
                    { JOBMANAGER.lock().unwrap().get_pid(pid).unwrap().jid },
                    pid,
                    signal
                );
                match fgpid {
                    -1 => {}
                    _ => LOCK.0.lock().unwrap().send(()).unwrap(),
                }
            }
            MessageQueue::Signal { signal } => {
                if fgpid != -1 {
                    kill(Pid::from_raw(-fgpid), Signal::try_from(signal).unwrap()).unwrap();
                }
            }
        }
    }
}
