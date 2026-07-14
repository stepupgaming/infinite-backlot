use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
}

impl ProcessSpec {
    pub fn command(&self) -> std::io::Result<Command> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            .envs(&self.env)
            .stdin(Stdio::null());
        if let Some(path) = &self.stdout_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            command.stdout(Stdio::from(std::fs::File::create(path)?));
        } else {
            command.stdout(Stdio::inherit());
        }
        if let Some(path) = &self.stderr_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            command.stderr(Stdio::from(std::fs::File::create(path)?));
        } else {
            command.stderr(Stdio::inherit());
        }
        Ok(command)
    }
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("runtime program does not exist: {0}")]
    MissingProgram(PathBuf),
    #[error("could not start runtime process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("runtime process {pid} exited with {status}")]
    Failed { pid: u32, status: ExitStatus },
}

#[derive(Debug)]
pub struct OwnedProcess {
    child: Child,
    pub spec: ProcessSpec,
}

impl OwnedProcess {
    pub fn spawn(spec: ProcessSpec) -> Result<Self, ProcessError> {
        // Bare command names are resolved through PATH by `Command`; explicit
        // relative/absolute paths must exist so preflight errors are precise.
        if spec.program.components().count() > 1 && !spec.program.exists() {
            return Err(ProcessError::MissingProgram(spec.program.clone()));
        }
        let child = spec.command()?.spawn()?;
        Ok(Self { child, spec })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, ProcessError> {
        Ok(self.child.try_wait()?)
    }

    pub fn wait(&mut self) -> Result<ExitStatus, ProcessError> {
        Ok(self.child.wait()?)
    }

    /// Terminate only the process tree rooted at the PID this project spawned.
    pub fn terminate_tree(&mut self) -> Result<ExitStatus, ProcessError> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(status);
        }
        #[cfg(windows)]
        {
            let pid = self.child.id().to_string();
            let _ = Command::new("taskkill")
                .args(["/PID", &pid, "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(not(windows))]
        self.child.kill()?;
        Ok(self.child.wait()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_worker_process_fails_before_spawn() {
        let spec = ProcessSpec {
            program: PathBuf::from("definitely-missing-backlot-worker/program.exe"),
            args: vec![],
            cwd: std::env::current_dir().unwrap(),
            env: BTreeMap::new(),
            stdout_path: None,
            stderr_path: None,
        };
        let error = OwnedProcess::spawn(spec).unwrap_err();
        assert!(matches!(error, ProcessError::MissingProgram(_)));
    }
}
