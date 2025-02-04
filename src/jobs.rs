use std::fmt::Display;

use nix::unistd::Pid;

pub trait Jobs {
    fn list(&self) -> String;
    fn add_job(&mut self, job: Job) -> Result<u32, ()>;
    fn remove_job(&mut self, pid: Pid) -> Result<(), ()>;
    fn get_pid(&self, pid: Pid) -> Result<&Job, ()>;
    #[allow(unused)]
    fn get_jid(&self, jid: u32) -> Result<&Job, ()>;
    fn get_pid_mut(&mut self, pid: Pid) -> Result<&mut Job, ()>;
    fn get_jid_mut(&mut self, jid: u32) -> Result<&mut Job, ()>;
    fn set_state(&mut self, pid: Pid, state: States) -> Result<&Job, ()>;
    fn set_fg(&mut self, pid: Pid);
    fn current(&mut self) -> Option<Pid>;
    fn next_jid(&mut self) -> u32;
}

#[derive(Debug)]
pub enum States {
    FG,
    BG,
    ST,
}

impl Display for States {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FG => write!(f, "Foreground"),
            Self::BG => write!(f, "Running"),
            Self::ST => write!(f, "Stopped"),
        }
    }
}

#[derive(Debug)]
pub struct Job {
    pub pid: Pid,
    pub jid: u32,
    pub state: States,
    pub cmd: String,
}

impl Display for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] ({}) {} {}",
            self.jid,
            self.pid,
            self.state,
            self.cmd.trim()
        )
    }
}

#[derive(Debug)]
pub struct JobManager {
    fg: Option<Pid>,
    jobs: Vec<Job>,
}

impl Jobs for JobManager {
    fn set_state(&mut self, pid: Pid, state: States) -> Result<&Job, ()> {
        if let Some(fg) = self.fg {
            if fg == pid {
                // match state {
                //     States::ST => self.fg = None,
                //     _ => {}
                // };
                self.fg = None;
            };
        }
        let index = match self.jobs.iter().position(|job| job.pid == (pid)) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };
        if let States::FG = state {
            self.fg = Some(pid);
        }

        // if States::FG == state {
        //     self.fg = Some(pid);
        // }
        self.jobs[index].state = state;
        Ok(&self.jobs[index])
    }
    fn set_fg(&mut self, pid: Pid) {
        self.fg = Some(pid);
    }
    fn remove_job(&mut self, pid: Pid) -> Result<(), ()> {
        let index = match self.jobs.iter().position(|job| job.pid == (pid)) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };

        if let Some(fg) = self.fg {
            if fg == pid {
                self.fg = None;
            }
        }

        self.jobs.remove(index);
        Ok(())
    }
    fn get_jid(&self, jid: u32) -> Result<&Job, ()> {
        let index = match self.jobs.iter().position(|job| job.jid == jid) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };
        Ok(&self.jobs[index])
    }
    fn get_pid(&self, pid: Pid) -> Result<&Job, ()> {
        let index = match self.jobs.iter().position(|job| job.pid == pid) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };
        Ok(&self.jobs[index])
    }
    fn get_jid_mut(&mut self, jid: u32) -> Result<&mut Job, ()> {
        let index = match self.jobs.iter().position(|job| job.jid == jid) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };
        Ok(&mut self.jobs[index])
    }
    fn get_pid_mut(&mut self, pid: Pid) -> Result<&mut Job, ()> {
        let index = match self.jobs.iter().position(|job| job.pid == pid) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };
        Ok(&mut self.jobs[index])
    }
    fn add_job(&mut self, job: Job) -> Result<u32, ()> {
        let jid = self.next_jid();
        if let States::FG = job.state {
            if let Some(_fg) = self.fg {
                return Err(());
            }
            self.fg = Some(job.pid);
            self.jobs.push(Job { jid, ..job });
            Ok(jid)
        } else {
            self.fg = None;
            self.jobs.push(Job { jid, ..job });
            Ok(jid)
        }
    }
    fn current(&mut self) -> Option<Pid> {
        self.fg
    }
    fn list(&self) -> String {
        let mut res: String = String::new();
        for job in &self.jobs {
            res += &format!("{}\n", job);
        }
        res
    }
    fn next_jid(&mut self) -> u32 {
        match self.jobs.last() {
            Some(job) => job.jid + 1,
            None => 1,
        }
    }
}

impl JobManager {
    pub fn new() -> Self {
        JobManager {
            fg: None,
            jobs: vec![],
        }
    }
}

impl Job {
    pub fn new(pid: Pid, state: States, cmd: String) -> Self {
        Self {
            pid,
            state,
            cmd,
            jid: u32::MAX,
        }
    }
}
