use crate::ai::python_protocol::{
    PythonOperation, PythonRequestEnvelope, PythonResponseEnvelope, PYTHON_PROTOCOL_VERSION,
};
use serde::Deserialize;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STDERR_TAIL_LIMIT: usize = 32 * 1024;

#[derive(Debug, Clone)]
pub struct PythonWorkerConfig {
    pub python_executable: PathBuf,
    pub worker_script: PathBuf,
    pub working_directory: PathBuf,
    pub python_path: PathBuf,
    pub environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    pub handshake_timeout: Duration,
    pub operation_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub queue_capacity: usize,
    pub max_consecutive_restarts: usize,
    pub max_operations_per_worker: usize,
    pub max_rss_growth_bytes: u64,
    pub max_handle_growth: u64,
}

impl Default for PythonWorkerConfig {
    fn default() -> Self {
        let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let executable_directory = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        let bundle_root = executable_directory
            .as_deref()
            .and_then(discover_bundled_python_root);
        let bundled_executable = bundle_root.as_deref().map(bundled_python_executable);
        let default_executable = bundled_executable.unwrap_or_else(|| {
            if cfg!(windows) {
                PathBuf::from("python")
            } else {
                PathBuf::from("python3")
            }
        });
        let python_executable = std::env::var_os("PYTHON_EXECUTABLE")
            .or_else(|| std::env::var_os("PYO3_PYTHON"))
            .map(PathBuf::from)
            .unwrap_or(default_executable);
        let worker_script = std::env::var_os("PYTHON_WORKER_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                bundle_root
                    .as_ref()
                    .map(|root| root.join("worker.py"))
                    .unwrap_or_else(|| source_root.join("python").join("worker.py"))
            });
        let python_path = bundle_root
            .clone()
            .unwrap_or_else(|| source_root.join("python"));
        let working_directory = bundle_root
            .as_ref()
            .and_then(|root| root.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| source_root.clone());
        Self {
            python_executable,
            worker_script,
            working_directory,
            python_path,
            environment: Vec::new(),
            handshake_timeout: Duration::from_secs(15),
            operation_timeout: Duration::from_secs(120),
            shutdown_timeout: Duration::from_secs(5),
            queue_capacity: 32,
            max_consecutive_restarts: 3,
            max_operations_per_worker: 250,
            max_rss_growth_bytes: 256 * 1024 * 1024,
            max_handle_growth: 32,
        }
    }
}

fn bundled_python_executable(bundle_root: &Path) -> PathBuf {
    if cfg!(windows) {
        bundle_root.join("runtime").join("python.exe")
    } else {
        bundle_root.join("runtime").join("bin").join("python3")
    }
}

fn discover_bundled_python_root(executable_directory: &Path) -> Option<PathBuf> {
    let mut candidates = vec![executable_directory.join("resources").join("python")];
    if let Some(contents) = executable_directory.parent() {
        candidates.push(contents.join("Resources").join("python"));
    }
    candidates
        .into_iter()
        .find(|root| root.join("worker.py").is_file() && bundled_python_executable(root).is_file())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonWorkerHandshake {
    pub event: String,
    pub protocol_version: String,
    pub worker_pid: u32,
    pub python_version: String,
    pub platform: String,
    pub ready: bool,
    pub bridge_error_class: Option<String>,
    pub pymupdf_version: Option<String>,
    pub pymupdf_pro_version: Option<String>,
    pub pro_version_compatible: bool,
    pub pro_package_available: bool,
    pub pro_import_error_class: Option<String>,
    pub operations: Vec<String>,
}

impl PythonWorkerHandshake {
    fn validate(&self) -> Result<(), PythonWorkerError> {
        if self.event != "handshake" {
            return Err(PythonWorkerError::InvalidHandshake(
                "first worker event was not a handshake".to_string(),
            ));
        }
        if self.protocol_version != PYTHON_PROTOCOL_VERSION {
            return Err(PythonWorkerError::InvalidHandshake(format!(
                "worker protocol {} does not match {}",
                self.protocol_version, PYTHON_PROTOCOL_VERSION
            )));
        }
        if self.worker_pid == 0 {
            return Err(PythonWorkerError::InvalidHandshake(
                "worker_pid must be non-zero".to_string(),
            ));
        }
        let expected = PythonOperation::ALL
            .iter()
            .map(|operation| {
                serde_json::to_value(operation)
                    .ok()
                    .and_then(|value| value.as_str().map(ToOwned::to_owned))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        if self.operations != expected {
            return Err(PythonWorkerError::InvalidHandshake(
                "worker operation catalog does not match Rust".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonWorkerError {
    Io(String),
    HandshakeTimeout,
    InvalidHandshake(String),
    InvalidRequest(String),
    InvalidResponse(String),
    WorkerExited {
        status: Option<i32>,
        stderr_tail: String,
    },
    ResponseTimeout,
    QueueFull,
    QueueClosed,
    ClientWaitTimeout,
    RestartLimit,
    RestartFailed(String),
}

impl fmt::Display for PythonWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Python worker I/O error: {error}"),
            Self::HandshakeTimeout => write!(formatter, "Python worker handshake timed out"),
            Self::InvalidHandshake(error) => write!(formatter, "invalid Python handshake: {error}"),
            Self::InvalidRequest(error) => write!(formatter, "invalid Python request: {error}"),
            Self::InvalidResponse(error) => write!(formatter, "invalid Python response: {error}"),
            Self::WorkerExited {
                status,
                stderr_tail,
            } => write!(
                formatter,
                "Python worker exited (status={status:?}, stderr_tail={stderr_tail:?})"
            ),
            Self::ResponseTimeout => write!(formatter, "Python operation timed out"),
            Self::QueueFull => write!(formatter, "Python worker request queue is full"),
            Self::QueueClosed => write!(formatter, "Python worker request queue is closed"),
            Self::ClientWaitTimeout => write!(formatter, "timed out waiting for Python result"),
            Self::RestartLimit => write!(formatter, "Python worker restart limit reached"),
            Self::RestartFailed(error) => {
                write!(formatter, "Python worker restart failed: {error}")
            }
        }
    }
}

impl std::error::Error for PythonWorkerError {}

struct PythonWorkerProcess {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<Result<String, String>>,
    stderr_tail: Arc<Mutex<String>>,
    handshake: PythonWorkerHandshake,
    operations_completed: usize,
    baseline_rss_bytes: Option<u64>,
    latest_rss_bytes: Option<u64>,
    baseline_open_handles: Option<u64>,
    latest_open_handles: Option<u64>,
}

impl PythonWorkerProcess {
    fn start(config: &PythonWorkerConfig) -> Result<Self, PythonWorkerError> {
        let mut command = Command::new(&config.python_executable);
        command
            .arg(&config.worker_script)
            .current_dir(&config.working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONUNBUFFERED", "1")
            .envs(config.environment.iter().cloned());
        let python_path = joined_python_path(&config.python_path)?;
        command.env("PYTHONPATH", python_path);

        let mut child = command
            .spawn()
            .map_err(|error| PythonWorkerError::Io(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PythonWorkerError::Io("worker stdin was not piped".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PythonWorkerError::Io("worker stdout was not piped".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PythonWorkerError::Io("worker stderr was not piped".to_string()))?;

        let (line_tx, line_rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("python-worker-stdout".to_string())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let mapped = line.map_err(|error| error.to_string());
                    if line_tx.send(mapped).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| PythonWorkerError::Io(error.to_string()))?;

        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let stderr_tail_reader = Arc::clone(&stderr_tail);
        thread::Builder::new()
            .name("python-worker-stderr".to_string())
            .spawn(move || collect_stderr_tail(stderr, &stderr_tail_reader))
            .map_err(|error| PythonWorkerError::Io(error.to_string()))?;

        let handshake_line = match line_rx.recv_timeout(config.handshake_timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                let _ = child.kill();
                return Err(PythonWorkerError::Io(error));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                return Err(PythonWorkerError::HandshakeTimeout);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child
                    .try_wait()
                    .ok()
                    .flatten()
                    .and_then(|status| status.code());
                let tail = locked_tail(&stderr_tail);
                return Err(PythonWorkerError::WorkerExited {
                    status,
                    stderr_tail: tail,
                });
            }
        };
        let handshake: PythonWorkerHandshake = serde_json::from_str(&handshake_line)
            .map_err(|error| PythonWorkerError::InvalidHandshake(error.to_string()))?;
        handshake.validate()?;

        Ok(Self {
            child,
            stdin,
            lines: line_rx,
            stderr_tail,
            handshake,
            operations_completed: 0,
            baseline_rss_bytes: None,
            latest_rss_bytes: None,
            baseline_open_handles: None,
            latest_open_handles: None,
        })
    }

    fn execute(
        &mut self,
        request: &PythonRequestEnvelope,
        default_timeout: Duration,
    ) -> Result<PythonResponseEnvelope, PythonWorkerError> {
        request
            .validate()
            .map_err(|error| PythonWorkerError::InvalidRequest(error.to_string()))?;
        if let Some(status) = self
            .child
            .try_wait()
            .map_err(|error| PythonWorkerError::Io(error.to_string()))?
        {
            return Err(PythonWorkerError::WorkerExited {
                status: status.code(),
                stderr_tail: locked_tail(&self.stderr_tail),
            });
        }

        let json = serde_json::to_string(request)
            .map_err(|error| PythonWorkerError::InvalidRequest(error.to_string()))?;
        self.stdin
            .write_all(json.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|error| PythonWorkerError::Io(error.to_string()))?;

        let timeout = request_timeout(request, default_timeout);
        let line = match self.lines.recv_timeout(timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => return Err(PythonWorkerError::Io(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.stop();
                return Err(PythonWorkerError::ResponseTimeout);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = self
                    .child
                    .try_wait()
                    .ok()
                    .flatten()
                    .and_then(|status| status.code());
                return Err(PythonWorkerError::WorkerExited {
                    status,
                    stderr_tail: locked_tail(&self.stderr_tail),
                });
            }
        };
        let response = PythonResponseEnvelope::from_json_exact(&line)
            .map_err(|error| PythonWorkerError::InvalidResponse(error.to_string()))?;
        response
            .validate_for(request)
            .map_err(|error| PythonWorkerError::InvalidResponse(error.to_string()))?;
        self.operations_completed = self.operations_completed.saturating_add(1);
        if let Some(rss) = response.metrics.rss_after_bytes {
            self.baseline_rss_bytes.get_or_insert(rss);
            self.latest_rss_bytes = Some(rss);
        }
        if let Some(handles) = response.metrics.open_handles_after {
            self.baseline_open_handles.get_or_insert(handles);
            self.latest_open_handles = Some(handles);
        }
        Ok(response)
    }

    fn should_recycle(&self, config: &PythonWorkerConfig) -> bool {
        if self.operations_completed >= config.max_operations_per_worker.max(1) {
            return true;
        }
        let rss_growth = self
            .baseline_rss_bytes
            .zip(self.latest_rss_bytes)
            .map(|(baseline, latest)| latest.saturating_sub(baseline))
            .unwrap_or(0);
        if rss_growth > config.max_rss_growth_bytes {
            return true;
        }
        let handle_growth = self
            .baseline_open_handles
            .zip(self.latest_open_handles)
            .map(|(baseline, latest)| latest.saturating_sub(baseline))
            .unwrap_or(0);
        handle_growth > config.max_handle_growth
    }

    fn stop(&mut self) {
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for PythonWorkerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct PythonWorkerSupervisor {
    config: PythonWorkerConfig,
    worker: Option<PythonWorkerProcess>,
    consecutive_restarts: usize,
}

impl PythonWorkerSupervisor {
    pub fn start(config: PythonWorkerConfig) -> Result<Self, PythonWorkerError> {
        let worker = PythonWorkerProcess::start(&config)?;
        Ok(Self {
            config,
            worker: Some(worker),
            consecutive_restarts: 0,
        })
    }

    pub fn handshake(&self) -> Option<&PythonWorkerHandshake> {
        self.worker.as_ref().map(|worker| &worker.handshake)
    }

    pub fn execute(
        &mut self,
        request: &PythonRequestEnvelope,
    ) -> Result<PythonResponseEnvelope, PythonWorkerError> {
        if self.worker.is_none() {
            self.restart()?;
        }
        let result = self
            .worker
            .as_mut()
            .ok_or(PythonWorkerError::RestartLimit)?
            .execute(request, self.config.operation_timeout);
        match result {
            Ok(response) => {
                self.consecutive_restarts = 0;
                let should_recycle = self
                    .worker
                    .as_ref()
                    .map(|worker| worker.should_recycle(&self.config))
                    .unwrap_or(false);
                if should_recycle {
                    if let Some(mut worker) = self.worker.take() {
                        worker.stop();
                    }
                    if let Err(error) = self.restart() {
                        tracing::warn!(
                            "Python worker completed an operation but could not recycle: {error}"
                        );
                    }
                }
                Ok(response)
            }
            Err(error) => {
                if let Some(mut worker) = self.worker.take() {
                    worker.stop();
                }
                let restart_result = self.restart();
                if let Err(restart_error) = restart_result {
                    return Err(PythonWorkerError::RestartFailed(format!(
                        "{error}; {restart_error}"
                    )));
                }
                // Never replay a failed request: a mutation may have reached the
                // filesystem before a crash. The replacement worker serves only
                // subsequent operations.
                Err(error)
            }
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker.stop();
        }
    }

    fn restart(&mut self) -> Result<(), PythonWorkerError> {
        if self.consecutive_restarts >= self.config.max_consecutive_restarts {
            return Err(PythonWorkerError::RestartLimit);
        }
        self.consecutive_restarts += 1;
        self.worker = Some(PythonWorkerProcess::start(&self.config)?);
        Ok(())
    }
}

impl Drop for PythonWorkerSupervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum WorkerCommand {
    Execute {
        request: PythonRequestEnvelope,
        reply: mpsc::SyncSender<Result<PythonResponseEnvelope, PythonWorkerError>>,
    },
    Shutdown {
        reply: mpsc::SyncSender<()>,
    },
}

#[derive(Clone)]
pub struct PythonWorkerClient {
    commands: mpsc::SyncSender<WorkerCommand>,
    operation_timeout: Duration,
}

pub struct PythonWorkerTicket {
    response: mpsc::Receiver<Result<PythonResponseEnvelope, PythonWorkerError>>,
    timeout: Duration,
}

impl PythonWorkerTicket {
    pub fn wait(self) -> Result<PythonResponseEnvelope, PythonWorkerError> {
        match self.response.recv_timeout(self.timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(PythonWorkerError::ClientWaitTimeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(PythonWorkerError::QueueClosed),
        }
    }
}

impl PythonWorkerClient {
    pub fn start(config: PythonWorkerConfig) -> Result<Self, PythonWorkerError> {
        let mut supervisor = PythonWorkerSupervisor::start(config.clone())?;
        let (command_tx, command_rx) = mpsc::sync_channel(config.queue_capacity.max(1));
        thread::Builder::new()
            .name("python-worker-supervisor".to_string())
            .spawn(move || {
                while let Ok(command) = command_rx.recv() {
                    match command {
                        WorkerCommand::Execute { request, reply } => {
                            let result = supervisor.execute(&request);
                            let _ = reply.send(result);
                        }
                        WorkerCommand::Shutdown { reply } => {
                            supervisor.shutdown();
                            let _ = reply.send(());
                            break;
                        }
                    }
                }
            })
            .map_err(|error| PythonWorkerError::Io(error.to_string()))?;
        Ok(Self {
            commands: command_tx,
            operation_timeout: config.operation_timeout,
        })
    }

    pub fn submit(
        &self,
        request: PythonRequestEnvelope,
    ) -> Result<PythonWorkerTicket, PythonWorkerError> {
        request
            .validate()
            .map_err(|error| PythonWorkerError::InvalidRequest(error.to_string()))?;
        let timeout = request_timeout(&request, self.operation_timeout)
            .saturating_add(Duration::from_secs(1));
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        match self.commands.try_send(WorkerCommand::Execute {
            request,
            reply: reply_tx,
        }) {
            Ok(()) => Ok(PythonWorkerTicket {
                response: reply_rx,
                timeout,
            }),
            Err(mpsc::TrySendError::Full(_)) => Err(PythonWorkerError::QueueFull),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(PythonWorkerError::QueueClosed),
        }
    }

    pub fn execute(
        &self,
        request: PythonRequestEnvelope,
    ) -> Result<PythonResponseEnvelope, PythonWorkerError> {
        self.submit(request)?.wait()
    }

    pub fn shutdown(&self, timeout: Duration) -> Result<(), PythonWorkerError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.commands
            .send(WorkerCommand::Shutdown { reply: reply_tx })
            .map_err(|_| PythonWorkerError::QueueClosed)?;
        reply_rx
            .recv_timeout(timeout)
            .map_err(|_| PythonWorkerError::ClientWaitTimeout)
    }
}

fn joined_python_path(primary: &Path) -> Result<std::ffi::OsString, PythonWorkerError> {
    let mut paths = vec![primary.to_path_buf()];
    if let Some(existing) = std::env::var_os("PYTHONPATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).map_err(|error| PythonWorkerError::Io(error.to_string()))
}

fn request_timeout(request: &PythonRequestEnvelope, default_timeout: Duration) -> Duration {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let deadline_timeout = Duration::from_millis(request.deadline_unix_ms.saturating_sub(now));
    if deadline_timeout.is_zero() {
        Duration::from_millis(1)
    } else {
        deadline_timeout.min(default_timeout)
    }
}

fn collect_stderr_tail<R: Read>(reader: R, tail: &Arc<Mutex<String>>) {
    let reader = BufReader::new(reader);
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(mut current) = tail.lock() {
            current.push_str(&line);
            current.push('\n');
            if current.len() > STDERR_TAIL_LIMIT {
                let split = current.len() - STDERR_TAIL_LIMIT;
                let safe_split = current
                    .char_indices()
                    .find_map(|(index, _)| (index >= split).then_some(index))
                    .unwrap_or(split);
                current.drain(..safe_split);
            }
        }
    }
}

fn locked_tail(tail: &Arc<Mutex<String>>) -> String {
    tail.lock().map(|value| value.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn discovers_adjacent_packaged_python_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let executable_directory = directory.path().join("app");
        let bundle_root = executable_directory.join("resources").join("python");
        let interpreter = bundled_python_executable(&bundle_root);
        std::fs::create_dir_all(interpreter.parent().unwrap()).unwrap();
        std::fs::write(bundle_root.join("worker.py"), "# worker").unwrap();
        std::fs::write(&interpreter, b"runtime").unwrap();
        assert_eq!(
            discover_bundled_python_root(&executable_directory),
            Some(bundle_root)
        );
    }

    #[test]
    fn discovers_macos_resources_layout_and_rejects_incomplete_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let executable_directory = directory
            .path()
            .join("Editor.app")
            .join("Contents")
            .join("MacOS");
        let bundle_root = executable_directory
            .parent()
            .unwrap()
            .join("Resources")
            .join("python");
        let interpreter = bundled_python_executable(&bundle_root);
        std::fs::create_dir_all(interpreter.parent().unwrap()).unwrap();
        std::fs::write(bundle_root.join("worker.py"), "# worker").unwrap();
        assert_eq!(discover_bundled_python_root(&executable_directory), None);
        std::fs::write(&interpreter, b"runtime").unwrap();
        assert_eq!(
            discover_bundled_python_root(&executable_directory),
            Some(bundle_root)
        );
    }

    fn write_blank_pdf(path: &Path) {
        use lopdf::{dictionary, Document, Object};

        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {},
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.save(path).unwrap();
    }

    fn request_for(
        operation: PythonOperation,
        payload: serde_json::Value,
    ) -> PythonRequestEnvelope {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        PythonRequestEnvelope::new(operation, Uuid::new_v4(), now, now + 30_000, None, payload)
            .unwrap()
    }

    fn ping_request() -> PythonRequestEnvelope {
        request_for(PythonOperation::Ping, json!({}))
    }

    #[test]
    fn real_worker_handshake_and_ping_match_protocol() {
        let mut supervisor = PythonWorkerSupervisor::start(PythonWorkerConfig::default()).unwrap();
        let handshake = supervisor.handshake().unwrap();
        assert_eq!(handshake.protocol_version, PYTHON_PROTOCOL_VERSION);
        assert_eq!(handshake.operations.len(), PythonOperation::ALL.len());
        let request = ping_request();
        let response = supervisor.execute(&request).unwrap();
        response.validate_for(&request).unwrap();
        assert_eq!(
            response.disposition,
            crate::ai::python_protocol::PythonDisposition::Succeeded
        );
        supervisor.shutdown();
    }

    #[test]
    fn bounded_client_delivers_exactly_one_correlated_response() {
        let config = PythonWorkerConfig {
            queue_capacity: 1,
            ..PythonWorkerConfig::default()
        };
        let client = PythonWorkerClient::start(config).unwrap();
        let request = ping_request();
        let operation_id = request.operation_id;
        let response = client.execute(request).unwrap();
        assert_eq!(response.operation_id, operation_id);
        client.shutdown(Duration::from_secs(5)).unwrap();
    }

    struct FaultHarness {
        _directory: tempfile::TempDir,
        config: PythonWorkerConfig,
        log_path: PathBuf,
    }

    impl FaultHarness {
        fn new(mode: &str, timeout: Duration) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let state_path = directory.path().join("fault.state");
            let log_path = directory.path().join("operations.log");
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let config = PythonWorkerConfig {
                worker_script: root.join("python").join("worker_fault_fixture.py"),
                environment: vec![
                    ("PYTHON_WORKER_FAULT_MODE".into(), mode.into()),
                    (
                        "PYTHON_WORKER_FAULT_STATE".into(),
                        state_path.as_os_str().to_owned(),
                    ),
                    (
                        "PYTHON_WORKER_FAULT_LOG".into(),
                        log_path.as_os_str().to_owned(),
                    ),
                ],
                operation_timeout: timeout,
                handshake_timeout: Duration::from_secs(5),
                max_operations_per_worker: 1_000,
                ..PythonWorkerConfig::default()
            };
            Self {
                _directory: directory,
                config,
                log_path,
            }
        }

        fn logged_ids(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log_path)
                .unwrap_or_default()
                .lines()
                .map(ToOwned::to_owned)
                .collect()
        }
    }

    fn assert_fault_restarts_without_replay(
        mode: &str,
        timeout: Duration,
        expected: fn(&PythonWorkerError) -> bool,
    ) {
        let harness = FaultHarness::new(mode, timeout);
        let mut supervisor = PythonWorkerSupervisor::start(harness.config.clone()).unwrap();
        let first = ping_request();
        let first_id = first.operation_id.to_string();
        let error = supervisor.execute(&first).unwrap_err();
        assert!(expected(&error), "unexpected fault result: {error}");

        let second = ping_request();
        let second_id = second.operation_id.to_string();
        let response = supervisor.execute(&second).unwrap();
        assert_eq!(response.operation_id, second.operation_id);
        let ids = harness.logged_ids();
        assert_eq!(ids.iter().filter(|id| *id == &first_id).count(), 1);
        assert_eq!(ids.iter().filter(|id| *id == &second_id).count(), 1);
        supervisor.shutdown();
    }

    #[test]
    fn crash_mid_operation_restarts_without_replay() {
        assert_fault_restarts_without_replay("crash_once", Duration::from_secs(2), |error| {
            matches!(error, PythonWorkerError::WorkerExited { .. })
        });
    }

    #[test]
    fn timeout_mid_operation_restarts_without_replay() {
        assert_fault_restarts_without_replay("hang_once", Duration::from_millis(150), |error| {
            matches!(error, PythonWorkerError::ResponseTimeout)
        });
    }

    #[test]
    fn malformed_stdout_restarts_without_replay() {
        assert_fault_restarts_without_replay("malformed_once", Duration::from_secs(2), |error| {
            matches!(error, PythonWorkerError::InvalidResponse(_))
        });
    }

    #[test]
    fn bounded_queue_rejects_overload() {
        let mut harness = FaultHarness::new("hang_once", Duration::from_millis(300));
        harness.config.queue_capacity = 1;
        let client = PythonWorkerClient::start(harness.config.clone()).unwrap();
        let first = client.submit(ping_request()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let second = client.submit(ping_request()).unwrap();
        assert!(matches!(
            client.submit(ping_request()),
            Err(PythonWorkerError::QueueFull)
        ));
        assert!(matches!(
            first.wait(),
            Err(PythonWorkerError::ResponseTimeout)
        ));
        assert_eq!(
            second.wait().unwrap().disposition,
            crate::ai::python_protocol::PythonDisposition::Succeeded
        );
        client.shutdown(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn operation_budget_recycles_worker_after_success() {
        let config = PythonWorkerConfig {
            max_operations_per_worker: 1,
            ..PythonWorkerConfig::default()
        };
        let mut supervisor = PythonWorkerSupervisor::start(config).unwrap();
        let first_pid = supervisor.handshake().unwrap().worker_pid;
        let response = supervisor.execute(&ping_request()).unwrap();
        assert_eq!(
            response.disposition,
            crate::ai::python_protocol::PythonDisposition::Succeeded
        );
        let replacement_pid = supervisor.handshake().unwrap().worker_pid;
        assert_ne!(first_pid, replacement_pid);
        supervisor.shutdown();
    }

    #[test]
    fn hundred_real_pdf_operations_close_handles_and_stay_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let pdf_path = directory.path().join("resource-stress.pdf");
        write_blank_pdf(&pdf_path);
        let config = PythonWorkerConfig {
            max_operations_per_worker: 25,
            max_rss_growth_bytes: 128 * 1024 * 1024,
            max_handle_growth: 16,
            ..PythonWorkerConfig::default()
        };
        let mut supervisor = PythonWorkerSupervisor::start(config).unwrap();
        let first_pid = supervisor.handshake().unwrap().worker_pid;
        let mut rss_samples = Vec::new();
        let mut handle_samples = Vec::new();

        for _ in 0..100 {
            let request = request_for(
                PythonOperation::GetTextBlocks,
                json!({"pdf_path": pdf_path, "page_num": 0}),
            );
            let operation_id = request.operation_id;
            let response = supervisor.execute(&request).unwrap();
            assert_eq!(response.operation_id, operation_id);
            assert_eq!(
                response.disposition,
                crate::ai::python_protocol::PythonDisposition::Succeeded
            );
            if let Some(rss) = response.metrics.rss_after_bytes {
                rss_samples.push(rss);
            }
            if let Some(handles) = response.metrics.open_handles_after {
                handle_samples.push(handles);
            }
        }

        assert_ne!(first_pid, supervisor.handshake().unwrap().worker_pid);
        if let (Some(minimum), Some(maximum)) = (rss_samples.iter().min(), rss_samples.iter().max())
        {
            assert!(maximum.saturating_sub(*minimum) < 128 * 1024 * 1024);
        }
        if let (Some(minimum), Some(maximum)) =
            (handle_samples.iter().min(), handle_samples.iter().max())
        {
            assert!(maximum.saturating_sub(*minimum) <= 16);
        }
        supervisor.shutdown();
    }

    #[test]
    fn hundred_operation_stress_stays_correlated_and_bounded() {
        let config = PythonWorkerConfig {
            max_operations_per_worker: 25,
            max_rss_growth_bytes: 128 * 1024 * 1024,
            max_handle_growth: 16,
            ..PythonWorkerConfig::default()
        };
        let mut supervisor = PythonWorkerSupervisor::start(config).unwrap();
        let first_pid = supervisor.handshake().unwrap().worker_pid;
        let mut rss_samples = Vec::new();
        let mut handle_samples = Vec::new();

        for _ in 0..100 {
            let request = ping_request();
            let operation_id = request.operation_id;
            let response = supervisor.execute(&request).unwrap();
            assert_eq!(response.operation_id, operation_id);
            if let Some(rss) = response.metrics.rss_after_bytes {
                rss_samples.push(rss);
            }
            if let Some(handles) = response.metrics.open_handles_after {
                handle_samples.push(handles);
            }
        }

        assert_ne!(first_pid, supervisor.handshake().unwrap().worker_pid);
        if let (Some(minimum), Some(maximum)) = (rss_samples.iter().min(), rss_samples.iter().max())
        {
            assert!(maximum.saturating_sub(*minimum) < 128 * 1024 * 1024);
        }
        if let (Some(minimum), Some(maximum)) =
            (handle_samples.iter().min(), handle_samples.iter().max())
        {
            assert!(maximum.saturating_sub(*minimum) <= 16);
        }
        supervisor.shutdown();
    }
}
