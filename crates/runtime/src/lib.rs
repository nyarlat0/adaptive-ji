//! Nonblocking realtime/worker bridge. Thread creation/destruction is explicitly
//! outside process(). Payloads are fixed-size Copy values, not heap-owning jobs.
use adaptive_ji_core::{
    realtime::{RetuneRequest, RetuneResult},
    tuning::{AdaptiveRationalOptimizer, adaptive::Proposal},
};
use crossbeam_queue::ArrayQueue;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub mod session;

struct Shared {
    requests: ArrayQueue<RetuneRequest>,
    results: ArrayQueue<RetuneResult>,
    latest: AtomicU64,
    accepted: AtomicU64,
    stop: AtomicBool,
    solved: AtomicU64,
    cancelled: AtomicU64,
}

pub struct OptimizerWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl OptimizerWorker {
    /// Initialization only. One worker per instance, never per voice.
    pub fn start(mut optimizer: AdaptiveRationalOptimizer) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            requests: ArrayQueue::new(2),
            results: ArrayQueue::new(2),
            latest: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            solved: AtomicU64::new(0),
            cancelled: AtomicU64::new(0),
        });
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("adaptive-ji optimizer".into())
            .spawn(move || {
                let mut epoch = 0;
                let mut pending: Option<(u64, Proposal)> = None;
                while !worker.stop.load(Ordering::Acquire) {
                    if pending.as_ref().is_some_and(|(generation, _)| {
                        *generation == worker.accepted.load(Ordering::Acquire)
                    }) {
                        optimizer.commit_proposal(pending.take().unwrap().1);
                    }
                    let Some(mut request) = worker.requests.pop() else {
                        // Only this non-realtime thread sleeps. No callback wakeup/lock.
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    };
                    if let Some(newer) = worker.requests.pop() {
                        request = newer;
                    }
                    // Recheck the acknowledgement after reading a new request. RT
                    // publishes it before publishing the request that supersedes it.
                    if pending.as_ref().is_some_and(|(generation, _)| {
                        *generation == worker.accepted.load(Ordering::Acquire)
                    }) {
                        optimizer.commit_proposal(pending.take().unwrap().1);
                    }
                    pending = None;
                    if request.epoch != epoch {
                        optimizer.reset_memory();
                        epoch = request.epoch;
                    }
                    let cancelled = || {
                        worker.stop.load(Ordering::Acquire)
                            || worker.latest.load(Ordering::Acquire) != request.generation
                    };
                    if let Some(proposal) =
                        optimizer.propose(&request.notes[..request.len], cancelled)
                    {
                        if cancelled() {
                            worker.cancelled.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        let result = RetuneResult::from_targets(request, &proposal.notes);
                        pending = Some((request.generation, proposal));
                        worker.results.force_push(result);
                        worker.solved.fetch_add(1, Ordering::Relaxed);
                    } else {
                        worker.cancelled.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Realtime-safe, bounded storage. Old queued work is replaced, not appended
    /// to an ever-growing backlog. The generation cancels an in-flight search.
    pub fn request(&self, request: RetuneRequest) {
        self.shared
            .latest
            .store(request.generation, Ordering::Release);
        self.shared.requests.force_push(request);
    }
    pub fn take_result(&self) -> Option<RetuneResult> {
        let first = self.shared.results.pop()?;
        Some(self.shared.results.pop().unwrap_or(first))
    }
    pub fn acknowledge(&self, generation: u64) {
        self.shared.accepted.store(generation, Ordering::Release);
    }
    pub fn counters(&self) -> (u64, u64) {
        (
            self.shared.solved.load(Ordering::Relaxed),
            self.shared.cancelled.load(Ordering::Relaxed),
        )
    }
}

impl Drop for OptimizerWorker {
    fn drop(&mut self) {
        // Owning frontends must only drop workers during non-realtime teardown.
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
