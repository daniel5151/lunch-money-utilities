use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::oneshot;

const BROADCAST_CAPACITY: usize = 8192;
const MAX_HISTORY_EVENTS: usize = 20_000;
const MAX_RETAINED_JOBS: usize = 30;

static JOB_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Deserialize)]
pub struct RunRequest {
    /// Command line arguments to pass to `lm-utils`, e.g. `["splitwise-sync", "sync", "window", "3 days"]`.
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
    Exit {
        code: Option<i32>,
        success: bool,
        killed: bool,
    },
    #[serde(rename = "error")]
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct JobSummary {
    pub job_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub status: String,
    pub exit_code: Option<i32>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

struct JobEntry {
    summary: JobSummary,
    history: Vec<RunEvent>,
    history_truncated: bool,
    tx: broadcast::Sender<RunEvent>,
    cancel_tx: Option<oneshot::Sender<()>>,
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
        let job_id = generate_job_id();
        let (tx, rx) = broadcast::channel::<RunEvent>(BROADCAST_CAPACITY);
        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();

        let exe = std::env::current_exe()?;
        let display_cmd = format_display_command(&args);

        // Build command
        let mut cmd = tokio::process::Command::new(&exe);
        // Explicitly pass config if present
        if self.config_path.exists() && !args.iter().any(|a| a == "--config" || a == "-c") {
            cmd.arg("--config").arg(&self.config_path);
        }
        cmd.args(&args);
        cmd.stdin(Stdio::null());
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

        let start_event = RunEvent::Start {
            command: display_cmd.clone(),
            job_id: job_id.clone(),
        };

        let summary = JobSummary {
            job_id: job_id.clone(),
            command: display_cmd,
            args,
            status: "running".to_string(),
            exit_code: None,
            started_at: now_epoch_ms(),
            finished_at: None,
        };

        {
            let mut jobs = self.jobs.lock().await;
            prune_old_jobs(&mut jobs);
            jobs.insert(
                job_id.clone(),
                JobEntry {
                    summary,
                    history: vec![start_event.clone()],
                    history_truncated: false,
                    tx: tx.clone(),
                    cancel_tx: Some(cancel_tx),
                },
            );
            let _ = tx.send(start_event);
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let jobs_map = self.jobs.clone();
        let job_id_clone = job_id.clone();

        tokio::spawn(async move {
            let mut stdout_lines = stdout.map(|s| BufReader::new(s).lines());
            let mut stderr_lines = stderr.map(|s| BufReader::new(s).lines());

            let mut stdout_done = stdout_lines.is_none();
            let mut stderr_done = stderr_lines.is_none();
            let mut was_killed = false;

            while !stdout_done || !stderr_done {
                tokio::select! {
                    _ = &mut cancel_rx, if !was_killed => {
                        was_killed = true;
                        let _ = child.kill().await;
                        push_event(
                            &jobs_map,
                            &job_id_clone,
                            RunEvent::Stderr("\x1b[1;33m[Job terminated by user]\x1b[0m".to_string()),
                        ).await;
                    }
                    res = async {
                        if let Some(ref mut lines) = stdout_lines {
                            lines.next_line().await
                        } else {
                            std::future::pending().await
                        }
                    }, if !stdout_done => {
                        match res {
                            Ok(Some(line)) => {
                                push_event(&jobs_map, &job_id_clone, RunEvent::Stdout(line)).await;
                            }
                            Ok(None) => {
                                stdout_done = true;
                            }
                            Err(e) => {
                                push_event(
                                    &jobs_map,
                                    &job_id_clone,
                                    RunEvent::Error(format!("Error reading stdout: {e}")),
                                ).await;
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
                                push_event(&jobs_map, &job_id_clone, RunEvent::Stderr(line)).await;
                            }
                            Ok(None) => {
                                stderr_done = true;
                            }
                            Err(e) => {
                                push_event(
                                    &jobs_map,
                                    &job_id_clone,
                                    RunEvent::Error(format!("Error reading stderr: {e}")),
                                ).await;
                                stderr_done = true;
                            }
                        }
                    }
                }
            }

            let status = if was_killed {
                child.wait().await
            } else {
                tokio::select! {
                    st = child.wait() => st,
                    _ = &mut cancel_rx => {
                        was_killed = true;
                        let _ = child.kill().await;
                        push_event(
                            &jobs_map,
                            &job_id_clone,
                            RunEvent::Stderr("\x1b[1;33m[Job terminated by user]\x1b[0m".to_string()),
                        ).await;
                        child.wait().await
                    }
                }
            };
            let (code, success) = match status {
                Ok(st) => (st.code(), st.success() && !was_killed),
                Err(e) => {
                    push_event(
                        &jobs_map,
                        &job_id_clone,
                        RunEvent::Error(format!("Process error: {e}")),
                    )
                    .await;
                    (None, false)
                }
            };

            let exit_event = RunEvent::Exit {
                code,
                success,
                killed: was_killed,
            };

            {
                let mut jobs = jobs_map.lock().await;
                if let Some(entry) = jobs.get_mut(&job_id_clone) {
                    entry.cancel_tx = None;
                    entry.summary.exit_code = code;
                    entry.summary.finished_at = Some(now_epoch_ms());
                    entry.summary.status = if was_killed {
                        "killed".to_string()
                    } else if success {
                        "success".to_string()
                    } else {
                        "error".to_string()
                    };
                    record_history(entry, exit_event.clone());
                    let _ = entry.tx.send(exit_event);
                }
                prune_old_jobs(&mut jobs);
            }
        });

        Ok((job_id, rx))
    }

    /// Returns `(history, Option<live_rx>)`. When a job has already finished, `live_rx` is `None`
    /// so SSE handlers can stream the history and close cleanly.
    pub async fn subscribe(
        &self,
        job_id: &str,
    ) -> Option<(Vec<RunEvent>, Option<broadcast::Receiver<RunEvent>>)> {
        let jobs = self.jobs.lock().await;
        jobs.get(job_id).map(|entry| {
            let rx = if entry.summary.finished_at.is_some() {
                None
            } else {
                Some(entry.tx.subscribe())
            };
            (entry.history.clone(), rx)
        })
    }

    pub async fn list_jobs(&self) -> Vec<JobSummary> {
        let jobs = self.jobs.lock().await;
        let mut summaries: Vec<JobSummary> = jobs.values().map(|e| e.summary.clone()).collect();
        summaries.sort_by_key(|b| std::cmp::Reverse(b.started_at));
        summaries
    }

    pub async fn kill_job(&self, job_id: &str) -> bool {
        let cancel_tx = {
            let mut jobs = self.jobs.lock().await;
            jobs.get_mut(job_id)
                .and_then(|entry| entry.cancel_tx.take())
        };

        if let Some(tx) = cancel_tx {
            let _ = tx.send(());
            return true;
        }
        false
    }
}

async fn push_event(
    jobs_map: &Arc<Mutex<HashMap<String, JobEntry>>>,
    job_id: &str,
    event: RunEvent,
) {
    let mut jobs = jobs_map.lock().await;
    if let Some(entry) = jobs.get_mut(job_id) {
        record_history(entry, event.clone());
        let _ = entry.tx.send(event);
    }
}

fn record_history(entry: &mut JobEntry, event: RunEvent) {
    if entry.history.len() < MAX_HISTORY_EVENTS {
        entry.history.push(event);
    } else if matches!(event, RunEvent::Exit { .. }) {
        // Always retain the terminal Exit event so late subscribers know the job finished.
        entry.history.push(event);
    } else if !entry.history_truncated {
        entry.history_truncated = true;
        entry.history.push(RunEvent::Stderr(format!(
            "\x1b[1;33m[History truncated after {MAX_HISTORY_EVENTS} lines; live output continues]\x1b[0m"
        )));
    }
}

fn prune_old_jobs(jobs: &mut HashMap<String, JobEntry>) {
    if jobs.len() < MAX_RETAINED_JOBS {
        return;
    }

    let mut finished: Vec<(String, u64)> = jobs
        .iter()
        .filter_map(|(id, entry)| entry.summary.finished_at.map(|ts| (id.clone(), ts)))
        .collect();
    finished.sort_by_key(|(_, ts)| *ts);

    let to_remove = jobs.len().saturating_sub(MAX_RETAINED_JOBS - 1);
    for (id, _) in finished.into_iter().take(to_remove) {
        jobs.remove(&id);
    }
}

fn format_display_command(args: &[String]) -> String {
    let mut out = String::from("lm-utils");
    for arg in args {
        out.push(' ');
        if arg.is_empty()
            || arg
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | '$' | '`' | ';'))
        {
            out.push('"');
            for ch in arg.chars() {
                if matches!(ch, '"' | '\\' | '$' | '`') {
                    out.push('\\');
                }
                out.push(ch);
            }
            out.push('"');
        } else {
            out.push_str(arg);
        }
    }
    out
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn generate_job_id() -> String {
    let seq = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}-{}", now.as_secs(), now.subsec_nanos(), seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_display_command_quotes_spaces() {
        let args = vec![
            "splitwise-sync".to_string(),
            "sync".to_string(),
            "window".to_string(),
            "3 days".to_string(),
            "--tag".to_string(),
            "my \"quoted\" tag".to_string(),
        ];
        assert_eq!(
            format_display_command(&args),
            r#"lm-utils splitwise-sync sync window "3 days" --tag "my \"quoted\" tag""#
        );
    }

    #[test]
    fn test_generate_job_id_unique() {
        let a = generate_job_id();
        let b = generate_job_id();
        assert_ne!(a, b);
    }
}
