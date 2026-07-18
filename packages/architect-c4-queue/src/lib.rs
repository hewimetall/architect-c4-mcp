//! Serial in-process write queue: all docs mutations run one-at-a-time.
//!
//! Not Redis / Docket / SQLite — a dedicated worker thread + channel.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

use architect_c4_domain::DomainError;
use parking_lot::Mutex;

type JobFn = Box<dyn FnOnce() -> Result<String, DomainError> + Send>;

struct Job {
    work: JobFn,
    reply: SyncSender<Result<String, DomainError>>,
}

/// Process-wide serial writer. `submit` blocks until the job finishes.
pub struct WriteQueue {
    tx: SyncSender<Job>,
    _worker: Mutex<Option<JoinHandle<()>>>,
}

impl WriteQueue {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel::<Job>(64);
        let handle = thread::Builder::new()
            .name("architect-c4-write-q".into())
            .spawn(move || worker_loop(rx))
            .expect("spawn write queue worker");
        Self {
            tx,
            _worker: Mutex::new(Some(handle)),
        }
    }

    /// Enqueue `work` and wait for the result JSON/string payload.
    pub fn submit<F>(&self, work: F) -> Result<String, DomainError>
    where
        F: FnOnce() -> Result<String, DomainError> + Send + 'static,
    {
        let (rtx, rrx) = mpsc::sync_channel(1);
        self.tx
            .send(Job {
                work: Box::new(work),
                reply: rtx,
            })
            .map_err(|_| DomainError::Message("write queue closed".into()))?;
        rrx.recv()
            .map_err(|_| DomainError::Message("write queue worker died".into()))?
    }
}

fn worker_loop(rx: Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        let out = (job.work)();
        let _ = job.reply.send(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn serializes_jobs() {
        let q = WriteQueue::start();
        let counter = Arc::new(AtomicUsize::new(0));
        let c1 = counter.clone();
        let c2 = counter.clone();
        let a = q.submit(move || {
            assert_eq!(c1.fetch_add(1, Ordering::SeqCst), 0);
            Ok("a".into())
        });
        let b = q.submit(move || {
            assert_eq!(c2.fetch_add(1, Ordering::SeqCst), 1);
            Ok("b".into())
        });
        assert_eq!(a.unwrap(), "a");
        assert_eq!(b.unwrap(), "b");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
