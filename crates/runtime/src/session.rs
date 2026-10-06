//! Format-independent block frontend. The host supplies a monotonic sample
//! cursor, not its possibly looping song position. Events must be sorted.
use crate::OptimizerWorker;
use adaptive_ji_core::{
    event::MidiEvent,
    realtime::{RealtimeCore, RetuneRequest, RetuneResult},
    settings::EngineSettings,
    tuning::AdaptiveRationalOptimizer,
};

pub struct Session {
    pub core: RealtimeCore,
    worker: OptimizerWorker,
    offline: AdaptiveRationalOptimizer,
    offline_epoch: u64,
    initialization: Vec<[u8; 3]>,
    initialize_pending: bool,
}

impl Session {
    /// Setup/configuration only: allocations and worker creation are allowed.
    pub fn new(settings: &EngineSettings, sample_rate: f64) -> Result<Self, String> {
        let prepared = settings.prepare()?;
        let core = RealtimeCore::new(
            &prepared,
            sample_rate,
            settings.optimizer.memory_reset_on_empty,
        )?;
        let initialization = core.initialization_messages();
        Ok(Self {
            core,
            worker: OptimizerWorker::start(prepared.optimizer).map_err(|e| e.to_string())?,
            offline: settings.prepare()?.optimizer,
            offline_epoch: 0,
            initialization,
            initialize_pending: true,
        })
    }
    fn submit(
        &mut self,
        request: RetuneRequest,
        offline: bool,
        mut emit: impl FnMut(u64, [u8; 3]),
    ) {
        if offline {
            if self.offline_epoch != request.epoch {
                self.offline.reset_memory();
                self.offline_epoch = request.epoch;
            }
            let proposal = self
                .offline
                .propose(&request.notes[..request.len], || false)
                .expect("uncancelled search");
            let result = RetuneResult::from_targets(request, &proposal.notes);
            if self.core.apply_result(request.sample, result, &mut emit) {
                self.offline.commit_proposal(proposal);
            }
        } else {
            self.worker.request(request);
        }
    }
    pub fn reset(&mut self, sample: u64, offline: bool, mut emit: impl FnMut(u64, [u8; 3])) {
        let request = self.core.reset(sample, &mut emit);
        self.submit(request, offline, emit);
        self.initialize_pending = true;
    }
    /// Automatic lifecycle cleanup, without sending channel-wide kills when
    /// the engine is already empty. Explicit Panic uses `reset()` instead.
    pub fn reset_for_host(
        &mut self,
        sample: u64,
        offline: bool,
        mut emit: impl FnMut(u64, [u8; 3]),
    ) {
        let request = self.core.reset_for_host(sample, &mut emit);
        self.submit(request, offline, emit);
        self.initialize_pending = true;
    }
    /// No allocations/locks/search in realtime mode, including initial notes.
    /// Offline mode deliberately solves synchronously and is deterministic.
    pub fn process(
        &mut self,
        start: u64,
        frames: u32,
        offline: bool,
        events: impl IntoIterator<Item = (u32, MidiEvent)>,
        mut emit: impl FnMut(u64, [u8; 3]),
    ) {
        if frames == 0 {
            return;
        }
        if self.initialize_pending {
            for message in &self.initialization {
                emit(start, *message);
            }
            self.initialize_pending = false;
        }
        self.core.advance_before(start, &mut emit);
        if !offline
            && let Some(result) = self.worker.take_result()
            && self.core.apply_result(start, result, &mut emit)
        {
            self.worker.acknowledge(result.generation);
        }
        let mut events = events.into_iter().peekable();
        while let Some((offset, _)) = events.peek() {
            let offset = (*offset).min(frames - 1);
            let sample = start + u64::from(offset);
            self.core.advance_before(sample, &mut emit);
            self.core.begin_batch();
            while events
                .peek()
                .is_some_and(|(o, _)| (*o).min(frames - 1) == offset)
            {
                let (_, event) = events.next().unwrap();
                self.core.event(sample, event, &mut emit);
            }
            if let Some(request) = self.core.finish_batch(sample) {
                self.submit(request, offline, &mut emit);
            }
            self.core.tick_at_boundary(sample, &mut emit);
        }
        self.core.advance_before(start + u64::from(frames), emit);
    }
    pub fn worker_counters(&self) -> (u64, u64) {
        self.worker.counters()
    }
}
