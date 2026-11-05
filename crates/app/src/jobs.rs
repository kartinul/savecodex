//! Web jobs: one solve or pack run, the events it sends while it works and the files it makes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::Notify;

const KEEP: Duration = Duration::from_secs(3600);

#[derive(Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    Docs { items: Vec<Item> },
    Reading { pages: usize },
    Found { items: Vec<Item> },
    Step { id: String, state: &'static str, note: String },
    Code { id: String, written: bool, code: String },
    Output { id: String, text: String },
    Shot { id: String, n: usize },
    Done { file: usize, name: String },
    Error { message: String },
    Finished,
}

#[derive(Serialize)]
struct Envelope<'a> {
    doc: &'a str,
    #[serde(flatten)]
    event: Event,
}

pub struct Job {
    events: Mutex<Vec<String>>,
    finished: Mutex<bool>,
    notify: Notify,
    shots: Mutex<Vec<Vec<u8>>>,
    docs: Mutex<Vec<(String, PathBuf)>>,
    task: Mutex<Option<tokio::task::AbortHandle>>,
    pub dir: tempfile::TempDir,
    created: Instant,
}

static JOBS: LazyLock<Mutex<HashMap<String, Arc<Job>>>> = LazyLock::new(Default::default);

impl Job {
    /// A new job, registered under a random id; jobs older than an hour are dropped on the way.
    pub fn create() -> anyhow::Result<(String, Arc<Job>)> {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let job = Arc::new(Job {
            events: Mutex::default(),
            finished: Mutex::new(false),
            notify: Notify::new(),
            shots: Mutex::default(),
            docs: Mutex::default(),
            task: Mutex::default(),
            dir: tempfile::tempdir()?,
            created: Instant::now(),
        });
        let mut jobs = JOBS.lock().unwrap();
        jobs.retain(|_, j| j.created.elapsed() < KEEP);
        jobs.insert(id.clone(), job.clone());
        Ok((id, job))
    }

    pub fn get(id: &str) -> Option<Arc<Job>> {
        JOBS.lock().unwrap().get(id).cloned()
    }

    /// Stops the run and forgets the job; its temp dir goes with the last reference.
    pub fn remove(id: &str) {
        if let Some(job) = JOBS.lock().unwrap().remove(id) {
            if let Some(task) = job.task.lock().unwrap().take() { task.abort() }
        }
    }

    pub fn set_task(&self, task: tokio::task::AbortHandle) {
        *self.task.lock().unwrap() = Some(task);
    }

    pub fn emitter(self: &Arc<Self>, doc: impl Into<String>) -> Emitter {
        Emitter { doc: doc.into(), job: self.clone() }
    }

    pub fn finish(&self) {
        self.push("", Event::Finished);
        *self.finished.lock().unwrap() = true;
        self.notify.notify_waiters();
    }

    fn push(&self, doc: &str, event: Event) {
        let json = serde_json::to_string(&Envelope { doc, event }).expect("events serialize");
        self.events.lock().unwrap().push(json);
        self.notify.notify_waiters();
    }

    /// Events from `from` on, waiting for new ones; None once the run finished and all were sent.
    pub async fn next(&self, from: usize) -> Option<Vec<String>> {
        loop {
            let notified = self.notify.notified();
            {
                let events = self.events.lock().unwrap();
                if events.len() > from { return Some(events[from..].to_vec()) }
                if *self.finished.lock().unwrap() { return None }
            }
            notified.await;
        }
    }

    pub fn shot(&self, n: usize) -> Option<Vec<u8>> {
        self.shots.lock().unwrap().get(n).cloned()
    }

    pub fn doc(&self, n: usize) -> Option<(String, PathBuf)> {
        self.docs.lock().unwrap().get(n).cloned()
    }
}

/// Sends one document's events into its job.
#[derive(Clone)]
pub struct Emitter {
    doc: String,
    job: Arc<Job>,
}

impl Emitter {
    pub fn send(&self, event: Event) {
        self.job.push(&self.doc, event);
    }

    pub fn step(&self, id: &str, state: &'static str, note: impl Into<String>) {
        self.send(Event::Step { id: id.into(), state, note: note.into() });
    }

    pub fn shot(&self, id: &str, png: Vec<u8>) {
        let n = {
            let mut shots = self.job.shots.lock().unwrap();
            shots.push(png);
            shots.len() - 1
        };
        self.send(Event::Shot { id: id.into(), n });
    }

    pub fn done(&self, name: String, path: PathBuf) {
        let file = {
            let mut docs = self.job.docs.lock().unwrap();
            docs.push((name.clone(), path));
            docs.len() - 1
        };
        self.send(Event::Done { file, name });
    }
}
