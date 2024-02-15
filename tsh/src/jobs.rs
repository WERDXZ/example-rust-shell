use std::fmt::Display;

use nix::unistd::Pid;

pub trait Jobs {
    fn list(&self) -> String;
    fn add_job(&mut self, job: Job) -> Result<(), ()>;
    fn remove_job(&mut self, pid: Pid) -> Result<(), ()>;
    fn get_pid_mut(&mut self, pid: Pid) -> Result<&mut Job, ()>;
    fn get_jid_mut(&mut self, jid: u32) -> Result<&mut Job, ()>;
    fn get_pid(&self, pid: Pid) -> Result<&Job, ()>;
    fn get_jid(&self, jid: u32) -> Result<&Job, ()>;
    // fn set_state_jid(&self, jid: u32, state: States) -> Result<(),()>;
    // fn set_state_pid(&self, pid: Pid, state: States) -> Result<(),()>;
    fn fg(&mut self) -> Result<&mut Job, ()>;
    fn next_jid(&mut self) -> u32;
}

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

pub struct JobManager {
    fg: Option<usize>,
    jobs: Vec<Job>,
    next_jid: u32,
}

impl Jobs for JobManager {
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
    fn remove_job(&mut self, pid: Pid) -> Result<(), ()> {
        let index = match self.jobs.iter().position(|job| job.pid == pid) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };

        match &self.fg {
            Some(fg) => {
                if self.jobs[*fg].pid == pid {
                    self.fg = None;
                }
            }
            None => {}
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
    fn add_job(&mut self, job: Job) -> Result<(), ()> {
        if let States::FG = job.state {
            if let Some(_) = self.fg {
                return Err(());
            }
            self.jobs.push(job);
            self.fg = Some(self.jobs.len()-1);
            Ok(())
        }else {
            self.jobs.push(job);
            Ok(())
        }
    }
    fn fg(&mut self) -> Result<&mut Job, ()> {
        match &mut self.fg {
            Some(fg) => Ok(&mut self.jobs[*fg]),
            None => Err(()),
        }
    }
    fn list(&self) -> String {
        let mut res: String = String::new();
        for job in &self.jobs {
            res += &format!("{}\n", job);
        }
        res
    }
    fn next_jid(&mut self) -> u32 {
        if self.jobs.len() == 0 {
            self.next_jid = 1;
            return self.next_jid;
        }
        let jid = self.next_jid;
        self.next_jid += 1;
        jid
    }
}

impl JobManager {
    pub fn new() -> Self {
        JobManager {
            next_jid: 1,
            fg: None,
            jobs: vec![],
        }
    }
}
