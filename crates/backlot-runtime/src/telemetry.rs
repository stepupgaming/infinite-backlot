use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PhaseTiming {
    pub phase: String,
    pub runtime_load_ms: u64,
    pub work_ms: u64,
    pub runtime_unload_ms: u64,
    pub total_ms: u64,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub child_pid: Option<u32>,
    pub model_revision: Option<String>,
    pub free_vram_before_mb: Option<u64>,
    pub free_vram_after_mb: Option<u64>,
    pub success: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RuntimeTelemetry {
    pub schema_version: u32,
    pub phases: Vec<PhaseTiming>,
}

fn global_phases() -> &'static Mutex<Vec<PhaseTiming>> {
    static PHASES: OnceLock<Mutex<Vec<PhaseTiming>>> = OnceLock::new();
    PHASES.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn clear_global_telemetry() {
    if let Ok(mut phases) = global_phases().lock() {
        phases.clear();
    }
}

pub fn record_global_phase(timing: PhaseTiming) {
    if let Ok(mut phases) = global_phases().lock() {
        phases.push(timing);
    }
}

pub fn snapshot_global_telemetry() -> RuntimeTelemetry {
    RuntimeTelemetry {
        schema_version: 2,
        phases: global_phases()
            .lock()
            .map(|phases| phases.clone())
            .unwrap_or_default(),
    }
}
