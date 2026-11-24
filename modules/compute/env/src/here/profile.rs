//! A job's profile, sampled where its stages run: its control group of its
//! own when it has one (the unit or scope `lnk-job-<id>` it runs in),
//! else the tree of processes under this one, read by the measure
//! contract's readers. Each sample is said after the mark (`sample
//! <json>`), so it reaches the job wherever the run is.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use link_env_contract::profile::{Reading, Sample};
use link_measure_contract::client::{group, now};

/// Where a sample is read.
#[derive(Clone, Debug)]
enum Source {
    Group(PathBuf),
    Tree(u32),
}

impl Source {
    /// This process's control group, when it's a job's own; else the
    /// tree under this process.
    fn here() -> Source {
        if let Some(group) = link_secretive::test_var("LNK_TEST_JOB_GROUP") {
            return Source::Group(group.into());
        }
        let me = std::process::id();
        match group::of_process(me, Path::new("/sys/fs/cgroup")) {
            Some(group) if is_jobs(&group) => Source::Group(group),
            _ => Source::Tree(me),
        }
    }

    fn read(&self, stage: &str) -> Sample {
        match self {
            Source::Group(dir) => {
                let io = group::group_io(dir);
                Sample {
                    at: now(),
                    stage: stage.to_string(),
                    reading: Reading::Group,
                    memory: group::group_memory(dir),
                    peak: group::group_memory_peak(dir),
                    cpu: group::group_cpu(dir),
                    read: io.map(|(r, _)| r),
                    written: io.map(|(_, w)| w),
                }
            }
            Source::Tree(root) => tree(*root, stage),
        }
    }
}

/// Whether control group `dir` is a job's own: its unit or scope.
fn is_jobs(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("lnk-job-"))
}

#[cfg(target_os = "linux")]
fn tree(root: u32, stage: &str) -> Sample {
    use link_measure_contract::client::process;
    let procs: Vec<process::Proc> = process::tree(&[root], process::alive, process::children)
        .into_iter()
        .filter_map(|pid| process::process(pid, true))
        .collect();
    Sample {
        at: now(),
        stage: stage.to_string(),
        reading: Reading::Tree,
        memory: Some(procs.iter().filter_map(|p| p.memory).sum()),
        peak: None,
        cpu: Some(procs.iter().map(|p| p.cpu).sum()),
        read: Some(procs.iter().map(|p| p.read).sum()),
        written: Some(procs.iter().map(|p| p.written).sum()),
    }
}

/// A Mac's tree isn't read here yet: every measure none.
#[cfg(not(target_os = "linux"))]
fn tree(_root: u32, stage: &str) -> Sample {
    Sample {
        at: now(),
        stage: stage.to_string(),
        reading: Reading::Tree,
        memory: None,
        peak: None,
        cpu: None,
        read: None,
        written: None,
    }
}

/// A sampler running beside the stages: every `every`, and when told at
/// each stage's edge ([`Sampler::edge`]), it says a sample of the stage
/// running.
pub(super) struct Sampler {
    source: Source,
    mark: String,
    stage: Arc<Mutex<String>>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    pub(super) fn start(mark: &str, every: Duration, first: &str) -> Sampler {
        let source = Source::here();
        let stage = Arc::new(Mutex::new(first.to_string()));
        let (stop, stopped) = mpsc::channel();
        let thread = {
            let (source, stage, mark) = (source.clone(), stage.clone(), mark.to_string());
            std::thread::spawn(move || {
                while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                    let name = stage.lock().map(|s| s.clone()).unwrap_or_default();
                    say(&mark, &source.read(&name));
                }
            })
        };
        Sampler {
            source,
            mark: mark.to_string(),
            stage,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// A sample at stage `name`'s start or end, `name` the stage running
    /// from now.
    pub(super) fn edge(&self, name: &str) {
        if let Ok(mut stage) = self.stage.lock() {
            *stage = name.to_string();
        }
        say(&self.mark, &self.source.read(name));
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn say(mark: &str, sample: &Sample) {
    if let Ok(json) = serde_json::to_string(sample) {
        super::say(mark, &format!("sample {json}"));
    }
}
