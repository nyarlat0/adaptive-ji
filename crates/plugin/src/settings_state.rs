//! Persisted sound configuration with a non-persisted change counter. Host
//! restore and GUI Apply use the same store; realtime only reads the counter.
use adaptive_ji_core::settings::EngineSettings;
use logic_nih_plug::params::persist::PersistentField;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Default)]
pub(super) struct SettingsState(Arc<SettingsData>);

#[derive(Default)]
struct SettingsData {
    value: Mutex<EngineSettings>,
    generation: AtomicU64,
}

impl SettingsState {
    pub fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> (u64, EngineSettings) {
        let value = self.0.value.lock().unwrap();
        (self.generation(), value.clone())
    }

    pub fn replace(&self, value: EngineSettings) -> u64 {
        let mut stored = self.0.value.lock().unwrap();
        *stored = value;
        self.0.generation.fetch_add(1, Ordering::AcqRel) + 1
    }
}

impl<'a> PersistentField<'a, EngineSettings> for SettingsState {
    fn set(&self, value: EngineSettings) {
        self.replace(value);
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&EngineSettings) -> R,
    {
        f(&self.0.value.lock().unwrap())
    }
}
