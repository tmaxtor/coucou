//! One background writer, with at most one newer snapshot waiting. The last
//! snapshot replaces pending work, never work already being written; writes
//! cannot finish out of order. Dropping the writer drains the last snapshot.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

struct Pending<T> {
    latest: Option<T>,
    closed: bool,
}

pub struct SnapshotWriter<T> {
    pending: Arc<(Mutex<Pending<T>>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> SnapshotWriter<T> {
    pub fn new(mut write: impl FnMut(T) + Send + 'static) -> Self {
        let pending = Arc::new((Mutex::new(Pending { latest: None, closed: false }), Condvar::new()));
        let worker = pending.clone();
        let thread = std::thread::spawn(move || loop {
            let snapshot = {
                let (lock, ready) = &*worker;
                let mut state = lock.lock().unwrap();
                while state.latest.is_none() && !state.closed {
                    state = ready.wait(state).unwrap();
                }
                match state.latest.take() {
                    Some(snapshot) => snapshot,
                    None => return,
                }
            };
            // Never hold the queue mutex while serializing, flushing or logging.
            write(snapshot);
        });
        Self { pending, thread: Some(thread) }
    }

    pub fn submit(&self, snapshot: T) {
        let (lock, ready) = &*self.pending;
        lock.lock().unwrap().latest = Some(snapshot);
        ready.notify_one();
    }
}

impl<T> Drop for SnapshotWriter<T> {
    fn drop(&mut self) {
        let (lock, ready) = &*self.pending;
        lock.lock().unwrap().closed = true;
        ready.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_blocked_disk_does_not_block_submission_and_shutdown_writes_the_latest() {
        let (started, waiting) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let (written, results) = mpsc::channel();
        let writer = SnapshotWriter::new(move |value| {
            if value == 1 {
                started.send(()).unwrap();
                blocked.recv().unwrap();
            }
            written.send(value).unwrap();
        });
        writer.submit(1);
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        // Both submissions finish while the first write is deliberately blocked.
        writer.submit(2);
        writer.submit(3);
        release.send(()).unwrap();
        drop(writer);
        assert_eq!(results.iter().collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn an_unused_writer_shuts_down_without_a_write() {
        let (written, results) = mpsc::channel::<()>();
        drop(SnapshotWriter::new(move |value| written.send(value).unwrap()));
        assert!(results.try_recv().is_err());
    }
}
