use std::fmt::Display;

use nix::unistd::Pid;

pub trait Jobs {
    fn list(&self) -> String;
    fn add_job(&mut self, job: Job) -> Result<(), ()>;
    fn remove_job(&mut self, pid: Pid) -> Result<(), ()>;
    fn get_pid(&self, pid: Pid) -> Result<&Job, ()>;
    fn get_jid(&self, jid: u32) -> Result<&Job, ()>;
    fn set_state(&mut self, pid:Pid, state:States)->Result<&Job,()>;
    fn fg(&mut self) -> Option<Pid>;
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
    next_jid: u32,
}

impl Jobs for JobManager {
    fn set_state(&mut self, pid:Pid, state:States)->Result<&Job,()> {
        if let Some(fg) = self.fg {
            if fg == pid {
                match state {
                    States::ST => {self.fg = None},
                    _ => {}
                };
            };
        } 
        let index = match self.jobs.iter().position(|job| job.pid == dbg!(pid)) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };
        self.jobs[index].state = state;
        Ok(&self.jobs[index])
        
    }
    fn remove_job(&mut self, pid: Pid) -> Result<(), ()> {
        dbg!(&self);
        let index = match self.jobs.iter().position(|job| job.pid == dbg!(pid)) {
            Some(index) => index,
            None => {
                return Err(());
            }
        };

        if let Some(fg) = self.fg{
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
    fn add_job(&mut self, job: Job) -> Result<(), ()> {
        dbg!(&self);
        let jid= self.next_jid();
        if let States::FG = job.state {
            if let Some(_fg) = self.fg {
                return Err(());
            }
            self.fg = Some(job.pid);
            self.jobs.push(Job{
                jid,
                ..job
            });
            Ok(())
        }else {
            self.fg = None;
            self.jobs.push(Job{
                jid,
                ..job
            });
            Ok(())
        }
    }
    fn fg(&mut self) -> Option<Pid> {
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
        // if self.jobs.len() == 0 {
        //     self.next_jid = 1;
        //     return self.next_jid;
        // }
        // let jid = self.next_jid;
        // self.next_jid += 1;
        // jid
        match self.jobs.last() {
            Some(job) => {job.jid+1},
            None => {1},
        }
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

impl Job {
    pub fn new(pid:Pid, state:States,cmd:String) -> Self {
        Self{
            pid,
            state,
            cmd,
            jid: u32::MAX
        }
    }
}
