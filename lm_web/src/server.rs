use std::convert::Infallible;
use std::path::Path as StdPath;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::extract::Multipart;
use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Json;
use axum::response::Response;
use axum::response::sse::Event;
use axum::response::sse::KeepAlive;
use axum::response::sse::Sse;
use axum::routing::get;
use axum::routing::post;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use serde::Deserialize;
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tower_http::cors::CorsLayer;

use crate::runner::JobManager;
use crate::runner::RunEvent;
use crate::runner::RunRequest;
use crate::schema::CommandSchema;

pub const INDEX_HTML: &str = include_str!("assets/index.html");

/// Maximum multipart upload payload size (100 MiB).
const MAX_UPLOAD_BODY_BYTES: usize = 100 * 1024 * 1024;
/// Uploaded temp files are deleted once they are older than 1 hour.
const UPLOAD_TTL: Duration = Duration::from_secs(60 * 60);
/// How often the background sweeper checks for expired uploads.
const UPLOAD_CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * 60);

static UPLOAD_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct AppState {
    pub schema: Arc<CommandSchema>,
    pub runner: JobManager,
    pub config_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub path: String,
    pub content: String,
    pub exists: bool,
    pub parse_error: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct UploadedFileInfo {
    pub name: String,
    pub path: String,
    pub size: usize,
}

pub fn upload_temp_dir() -> PathBuf {
    std::env::temp_dir().join("lm_utils_uploads")
}

pub fn create_router(state: AppState) -> Router {
    let upload_route =
        post(upload_file_handler).layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES));

    Router::new()
        .route("/", get(serve_index))
        .route("/api/health", get(health_handler))
        .route("/api/schema", get(get_schema))
        .route(
            "/api/config",
            get(get_config_handler).put(update_config_handler),
        )
        .route("/api/jobs", get(list_jobs_handler))
        .route("/api/run", post(run_command_handler))
        .route("/api/events/{job_id}", get(get_events_handler))
        .route("/api/kill/{job_id}", post(kill_job_handler))
        .route("/api/upload", upload_route)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn serve_index() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(INDEX_HTML),
    )
        .into_response()
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn get_schema(State(state): State<AppState>) -> Json<CommandSchema> {
    Json((*state.schema).clone())
}

async fn list_jobs_handler(State(state): State<AppState>) -> Response {
    let jobs = state.runner.list_jobs().await;
    Json(jobs).into_response()
}

fn validate_config_content(content: &str) -> (Option<String>, Vec<String>) {
    if content.trim().is_empty() {
        return (None, Vec::new());
    }
    match content.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => {
            let mut warnings = Vec::new();
            if let Err(err) = lm_common::config::common_section(&doc) {
                warnings.push(format!("[common] validation warning: {err}"));
            }
            (None, warnings)
        }
        Err(e) => (Some(format!("Invalid TOML: {e}")), Vec::new()),
    }
}

async fn get_config_handler(State(state): State<AppState>) -> Response {
    let path = &state.config_path;
    let exists = tokio::fs::try_exists(path).await.unwrap_or(false);
    let content = if exists {
        match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        serde_json::json!({ "error": format!("Failed to read config file: {e}") }),
                    ),
                )
                    .into_response();
            }
        }
    } else {
        String::new()
    };

    let (parse_error, warnings) = validate_config_content(&content);

    Json(ConfigResponse {
        path: path.to_string_lossy().to_string(),
        content,
        exists,
        parse_error,
        warnings,
    })
    .into_response()
}

async fn update_config_handler(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> Response {
    // Validate TOML syntax strictly
    let doc = match req.content.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid TOML: {e}") })),
            )
                .into_response();
        }
    };

    let mut warnings = Vec::new();
    if !req.content.trim().is_empty() {
        if let Err(err) = lm_common::config::common_section(&doc) {
            warnings.push(format!("[common] validation warning: {err}"));
        }
    }

    if let Some(parent) = state.config_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
    }

    match tokio::fs::write(&state.config_path, &req.content).await {
        Ok(_) => Json(serde_json::json!({
            "success": true,
            "path": state.config_path.to_string_lossy().to_string(),
            "warnings": warnings
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to save config: {e}") })),
        )
            .into_response(),
    }
}

async fn run_command_handler(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Response {
    match state.runner.run_command(req.args).await {
        Ok((job_id, _rx)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "job_id": job_id })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn get_events_handler(State(state): State<AppState>, Path(job_id): Path<String>) -> Response {
    let sub_opt = state.runner.subscribe(&job_id).await;
    match sub_opt {
        Some((history, rx_opt)) => {
            let history_stream =
                futures_util::stream::iter(history).filter_map(|event| async move {
                    let json = serde_json::to_string(&event).ok()?;
                    Some(Ok::<Event, Infallible>(Event::default().data(json)))
                });

            let combined_stream: BoxStream<'static, Result<Event, Infallible>> = match rx_opt {
                Some(rx) => {
                    let live_stream = BroadcastStream::new(rx).filter_map(|res| async move {
                        let event = match res {
                            Ok(event) => event,
                            Err(BroadcastStreamRecvError::Lagged(skipped)) => RunEvent::Error(
                                format!("Output stream lagged: skipped {skipped} lines"),
                            ),
                        };
                        let json = serde_json::to_string(&event).ok()?;
                        Some(Ok::<Event, Infallible>(Event::default().data(json)))
                    });
                    history_stream.chain(live_stream).boxed()
                }
                None => history_stream.boxed(),
            };

            Sse::new(combined_stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Job not found or expired" })),
        )
            .into_response(),
    }
}

async fn kill_job_handler(State(state): State<AppState>, Path(job_id): Path<String>) -> Response {
    let killed = state.runner.kill_job(&job_id).await;
    Json(serde_json::json!({ "killed": killed })).into_response()
}

async fn upload_file_handler(mut multipart: Multipart) -> Response {
    let temp_dir = upload_temp_dir();
    if let Err(e) = tokio::fs::create_dir_all(&temp_dir).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({ "error": format!("Failed to prepare upload directory: {e}") }),
            ),
        )
            .into_response();
    }

    let mut uploaded: Vec<UploadedFileInfo> = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Failed to read multipart upload: {e}") })),
                )
                    .into_response();
            }
        };

        let raw_name = field.file_name().unwrap_or("upload.tmp").to_string();
        let safe_name = sanitize_filename(&raw_name);
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Failed to read uploaded file '{safe_name}': {e}") })),
                )
                    .into_response();
            }
        };

        let size = bytes.len();
        let dest = temp_dir.join(format!("{}_{}", unique_upload_prefix(), safe_name));
        if let Err(e) = tokio::fs::write(&dest, bytes).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to save file '{safe_name}': {e}") })),
            )
                .into_response();
        }

        uploaded.push(UploadedFileInfo {
            name: safe_name,
            path: dest.to_string_lossy().to_string(),
            size,
        });
    }

    if uploaded.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No file uploaded" })),
        )
            .into_response();
    }

    let first_path = uploaded[0].path.clone();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "path": first_path,
            "files": uploaded
        })),
    )
        .into_response()
}

fn sanitize_filename(raw: &str) -> String {
    let base = StdPath::new(raw)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("upload.tmp");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "upload.tmp".to_string()
    } else {
        cleaned
    }
}

fn unique_upload_prefix() -> String {
    let seq = UPLOAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}_{}_{}", now.as_secs(), now.subsec_nanos(), seq)
}

/// Remove regular files in `dir` whose last-modified timestamp is older than `ttl`.
pub async fn cleanup_expired_uploads(dir: &StdPath, ttl: Duration) -> usize {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return 0,
    };

    let now = SystemTime::now();
    let mut removed = 0;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if let Ok(age) = now.duration_since(modified) {
            if age >= ttl && tokio::fs::remove_file(entry.path()).await.is_ok() {
                removed += 1;
            }
        }
    }

    removed
}

pub async fn run_server(
    host: String,
    port: u16,
    open_browser: bool,
    schema: CommandSchema,
    config_path: PathBuf,
) -> anyhow::Result<()> {
    let runner = JobManager::new(config_path.clone());
    let state = AppState {
        schema: Arc::new(schema),
        runner,
        config_path,
    };

    // Periodically clean up uploaded temp files older than 1 hour.
    let uploads_dir = upload_temp_dir();
    tokio::spawn(async move {
        let _ = cleanup_expired_uploads(&uploads_dir, UPLOAD_TTL).await;
        let mut interval = tokio::time::interval(UPLOAD_CLEANUP_INTERVAL);
        interval.tick().await; // skip immediate first tick
        loop {
            interval.tick().await;
            let _ = cleanup_expired_uploads(&uploads_dir, UPLOAD_TTL).await;
        }
    });

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    let local_addr = listener.local_addr()?;
    let display_url = format!("http://{local_addr}");
    let browser_url = if local_addr.ip().is_unspecified() {
        format!("http://127.0.0.1:{}", local_addr.port())
    } else {
        display_url.clone()
    };

    anstream::println! {
        "{}\n🚀 Lunch Money Utilities Web GUI running at: {}\n{}",
        lm_common::style::STYLE_HEADER,
        display_url,
        lm_common::style::STYLE_HEADER
    };

    if open_browser {
        let _ = open::that(&browser_url);
    }

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("payslip_2026.pdf"), "payslip_2026.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("/tmp/dir/my file.pdf"), "my file.pdf");
        assert_eq!(sanitize_filename(""), "upload.tmp");
        assert_eq!(sanitize_filename(".."), "upload.tmp");
    }

    #[tokio::test]
    async fn test_cleanup_expired_uploads() {
        let dir =
            std::env::temp_dir().join(format!("lm_web_cleanup_test_{}", unique_upload_prefix()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let file = dir.join("sample.pdf");
        tokio::fs::write(&file, b"pdf-bytes").await.unwrap();

        // TTL of 1 hour should NOT remove a just-created file.
        let removed_fresh = cleanup_expired_uploads(&dir, Duration::from_secs(3600)).await;
        assert_eq!(removed_fresh, 0);
        assert!(file.exists());

        // TTL of 0 should remove it immediately.
        let removed_old = cleanup_expired_uploads(&dir, Duration::ZERO).await;
        assert_eq!(removed_old, 1);
        assert!(!file.exists());

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
