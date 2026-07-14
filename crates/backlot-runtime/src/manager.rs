use crate::process::{OwnedProcess, ProcessError, ProcessSpec};
use crate::telemetry::{record_global_phase, PhaseTiming, RuntimeTelemetry};
use std::process::ExitStatus;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Gemma,
    Gepard,
    Parakeet,
    Kimodo,
}

impl RuntimeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gemma => "llm_authoring",
            Self::Gepard => "tts",
            Self::Parakeet => "speech_alignment",
            Self::Kimodo => "kimodo_generation",
        }
    }
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime {requested:?} cannot start while {active:?} owns the model slot")]
    Busy {
        requested: RuntimeKind,
        active: RuntimeKind,
    },
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error("no runtime is active")]
    NotActive,
}

struct ActiveRuntime {
    kind: RuntimeKind,
    process: OwnedProcess,
    started: Instant,
    timing: PhaseTiming,
}

pub struct ModelRuntimeManager {
    active: Option<ActiveRuntime>,
    telemetry: RuntimeTelemetry,
}

impl Default for ModelRuntimeManager {
    fn default() -> Self {
        Self {
            active: None,
            telemetry: RuntimeTelemetry {
                schema_version: 2,
                phases: vec![],
            },
        }
    }
}

impl ModelRuntimeManager {
    pub fn active_kind(&self) -> Option<RuntimeKind> {
        self.active.as_ref().map(|active| active.kind)
    }

    pub fn start(
        &mut self,
        kind: RuntimeKind,
        spec: ProcessSpec,
        model_revision: Option<String>,
    ) -> Result<u32, RuntimeError> {
        if let Some(active) = &self.active {
            return Err(RuntimeError::Busy {
                requested: kind,
                active: active.kind,
            });
        }
        let load = Instant::now();
        let free_vram_before_mb = query_free_vram_mb();
        let process = OwnedProcess::spawn(spec)?;
        let pid = process.id();
        self.active = Some(ActiveRuntime {
            kind,
            process,
            started: Instant::now(),
            timing: PhaseTiming {
                phase: kind.label().into(),
                runtime_load_ms: load.elapsed().as_millis() as u64,
                child_pid: Some(pid),
                model_revision,
                free_vram_before_mb,
                ..Default::default()
            },
        });
        Ok(pid)
    }

    pub fn mark_work_complete(
        &mut self,
        cache_hits: u32,
        cache_misses: u32,
    ) -> Result<(), RuntimeError> {
        let active = self.active.as_mut().ok_or(RuntimeError::NotActive)?;
        active.timing.work_ms = active.started.elapsed().as_millis() as u64;
        active.timing.cache_hits = cache_hits;
        active.timing.cache_misses = cache_misses;
        Ok(())
    }

    /// Wait for a finite batch worker to exit without surrendering ownership of
    /// its PID. The subsequent `stop` call records unload telemetry and is safe
    /// when the child has already exited.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<bool, RuntimeError> {
        let active = self.active.as_mut().ok_or(RuntimeError::NotActive)?;
        let started = Instant::now();
        loop {
            if let Some(status) = active.process.try_wait()? {
                active.timing.success = status.success();
                return Ok(status.success());
            }
            if started.elapsed() >= timeout {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn poll_exit(&mut self) -> Result<Option<ExitStatus>, RuntimeError> {
        let active = self.active.as_mut().ok_or(RuntimeError::NotActive)?;
        active.process.try_wait().map_err(RuntimeError::from)
    }

    pub fn stop(&mut self) -> Result<PhaseTiming, RuntimeError> {
        let mut active = self.active.take().ok_or(RuntimeError::NotActive)?;
        if active.timing.work_ms == 0 {
            active.timing.work_ms = active.started.elapsed().as_millis() as u64;
        }
        let unload = Instant::now();
        let status = active.process.terminate_tree()?;
        active.timing.runtime_unload_ms = unload.elapsed().as_millis() as u64;
        active.timing.free_vram_after_mb = query_free_vram_mb();
        active.timing.total_ms =
            active.timing.runtime_load_ms + active.timing.work_ms + active.timing.runtime_unload_ms;
        active.timing.success = status.success() || status.code().is_none();
        self.telemetry.phases.push(active.timing.clone());
        record_global_phase(active.timing.clone());
        Ok(active.timing)
    }

    pub fn telemetry(&self) -> &RuntimeTelemetry {
        &self.telemetry
    }
}

fn query_free_vram_mb() -> Option<u64> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().parse::<u64>().ok())
}

impl Drop for ModelRuntimeManager {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            let _ = active.process.terminate_tree();
        }
    }
}
