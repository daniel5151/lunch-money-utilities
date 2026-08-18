use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Deserialize)]
pub struct RunRequest {
    /// Command line arguments to pass to `lm-utils`, e.g. `["splitwise-sync", "sync", "window", "--window", "3 days"]`.
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum RunEvent {
    #[serde(rename = "start")]
    Start { command: String, job_id: String },
    #[serde(rename = "stdout")]
    Stdout(String),
    #[serde(rename = "stderr")]
    Stderr(String),
    #[serde(rename = "exit")]
    Exit { code: Option<i32>, success: bool },
    #[serde(rename = "error")]
    Error(String),
}

struct JobEntry {
    pid: Option<u32>,
    history: Vec<RunEvent>,
    tx: broadcast::Sender<RunEvent>,
}

#[derive(Clone)]
pub struct JobManager {
    jobs: Arc<Mutex<HashMap<String, JobEntry>>>,
    config_path: PathBuf,
}

impl JobManager {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            config_path,
        }
    }

    pub async fn run_command(
        &self,
        args: Vec<String>,
    ) -> anyhow::Result<(String, broadcast::Receiver<RunEvent>)> {
        let job_id = uuid_or_timestamp();
        let (tx, rx) = broadcast::channel::<RunEvent>(1024);

        let exe = std::env::current_exe()?;
        let display_cmd = format!("lm-utils {}", args.join(" "));

        // Build command
        let mut cmd = tokio::process::Command::new(&exe);
        // Explicitly pass config if present
        if self.config_path.exists() && !args.iter().any(|a| a == "--config" || a == "-c") {
            cmd.arg("--config").arg(&self.config_path);
        }
        cmd.args(&args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.env("TERM", "xterm-256color");
        cmd.env("CLICOLOR_FORCE", "1");

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = tx.send(RunEvent::Error(format!("Failed to spawn process: {e}")));
                return Err(e.into());
            }
        };

        let pid = child.id();
        let start_event = RunEvent::Start {
            command: display_cmd,
            job_id: job_id.clone(),
        };

        {
            let mut jobs = self.jobs.lock().await;
            jobs.insert(
                job_id.clone(),
                JobEntry {
                    pid,
                    history: vec![start_event.clone()],
                    tx: tx.clone(),
                },
            );
        }

        let _ = tx.send(start_event);

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let jobs_map = self.jobs.clone();
        let job_id_clone = job_id.clone();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            let mut stdout_lines = stdout.map(|s| BufReader::new(s).lines());
            let mut stderr_lines = stderr.map(|s| BufReader::new(s).lines());

            let mut stdout_done = stdout_lines.is_none();
            let mut stderr_done = stderr_lines.is_none();

            loop {
                tokio::select! {
                    res = async {
                        if let Some(ref mut lines) = stdout_lines {
                            lines.next_line().await
                        } else {
                            std::future::pending().await
                        }
                    }, if !stdout_done => {
                        match res {
                            Ok(Some(line)) => {
                                let event = RunEvent::Stdout(line);
                                let mut jobs = jobs_map.lock().await;
                                if let Some(entry) = jobs.get_mut(&job_id_clone) {
                                    entry.history.push(event.clone());
                                }
                                let _ = tx_clone.send(event);
                            }
                            Ok(None) => {
                                stdout_done = true;
                            }
                            Err(e) => {
                                let event = RunEvent::Error(format!("Error reading stdout: {e}"));
                                let mut jobs = jobs_map.lock().await;
                                if let Some(entry) = jobs.get_mut(&job_id_clone) {
                                    entry.history.push(event.clone());
                                }
                                let _ = tx_clone.send(event);
                                stdout_done = true;
                            }
                        }
                    }
                    res = async {
                        if let Some(ref mut lines) = stderr_lines {
                            lines.next_line().await
                        } else {
                            std::future::pending().await
                        }
                    }, if !stderr_done => {
                        match res {
                            Ok(Some(line)) => {
                                let event = RunEvent::Stderr(line);
                                let mut jobs = jobs_map.lock().await;
                                if let Some(entry) = jobs.get_mut(&job_id_clone) {
                                    entry.history.push(event.clone());
                                }
                                let _ = tx_clone.send(event);
                            }
                            Ok(None) => {
                                stderr_done = true;
                            }
                            Err(e) => {
                                let event = RunEvent::Error(format!("Error reading stderr: {e}"));
                                let mut jobs = jobs_map.lock().await;
                                if let Some(entry) = jobs.get_mut(&job_id_clone) {
                                    entry.history.push(event.clone());
                                }
                                let _ = tx_clone.send(event);
                                stderr_done = true;
                            }
                        }
                    }
                    else => {
                        break;
                    }
                }
            }

            let status = child.wait().await;
            let (code, success) = match status {
                Ok(st) => (st.code(), st.success()),
                Err(e) => {
                    let event = RunEvent::Error(format!("Process error: {e}"));
                    let mut jobs = jobs_map.lock().await;
                    if let Some(entry) = jobs.get_mut(&job_id_clone) {
                        entry.history.push(event.clone());
                    }
                    let _ = tx_clone.send(event);
                    (None, false)
                }
            };

            let exit_event = RunEvent::Exit { code, success };
            {
                let mut jobs = jobs_map.lock().await;
                if let Some(entry) = jobs.get_mut(&job_id_clone) {
                    entry.history.push(exit_event.clone());
                }
            }
            let _ = tx_clone.send(exit_event);

            // Allow client to connect and receive final events before pruning
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut jobs = jobs_map.lock().await;
            jobs.remove(&job_id_clone);
        });

        Ok((job_id, rx))
    }

    pub async fn subscribe(
        &self,
        job_id: &str,
    ) -> Option<(Vec<RunEvent>, broadcast::Receiver<RunEvent>)> {
        let jobs = self.jobs.lock().await;
        jobs.get(job_id)
            .map(|entry| (entry.history.clone(), entry.tx.subscribe()))
    }

    pub async fn kill_job(&self, job_id: &str) -> bool {
        let pid_opt = {
            let jobs = self.jobs.lock().await;
            jobs.get(job_id).and_then(|entry| entry.pid)
        };

        if let Some(pid) = pid_opt {
            #[cfg(unix)]
            {
                let _ = tokio::process::Command::new("kill")
                    .arg("-15")
                    .arg(pid.to_string())
                    .status()
                    .await;
                return true;
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                return false;
            }
        }
        false
    }
}

fn uuid_or_timestamp() -> String {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}", now.as_secs(), now.subsec_nanos())
}
