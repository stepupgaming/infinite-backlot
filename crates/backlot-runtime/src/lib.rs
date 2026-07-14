//! Project-owned model process lifecycle for Infinite Backlot.

pub mod gepard;
pub mod kimodo;
pub mod llama;
pub mod manager;
pub mod parakeet;
pub mod process;
pub mod telemetry;

pub use manager::{ModelRuntimeManager, RuntimeError, RuntimeKind};
pub use process::ProcessSpec;
pub use telemetry::{
    clear_global_telemetry, snapshot_global_telemetry, PhaseTiming, RuntimeTelemetry,
};
