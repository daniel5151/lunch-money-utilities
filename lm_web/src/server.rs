use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::Multipart;
use axum::extract::Path;
use axum::extract::State;
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
use serde::Deserialize;
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::CorsLayer;

use crate::runner::JobManager;
use crate::runner::RunRequest;
use crate::schema::CommandSchema;

pub const INDEX_HTML: &str = include_str!("assets/index.html");

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
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub content: String,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/api/schema", get(get_schema))
        .route(
            "/api/config",
            get(get_config_handler).put(update_config_handler),
        )
        .route("/api/run", post(run_command_handler))
        .route("/api/events/{job_id}", get(get_events_handler))
        .route("/api/kill/{job_id}", post(kill_job_handler))
        .route("/api/upload", post(upload_file_handler))
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

async fn get_schema(State(state): State<AppState>) -> Json<CommandSchema> {
    Json((*state.schema).clone())
}

async fn get_config_handler(State(state): State<AppState>) -> Response {
    let path = &state.config_path;
    let (content, exists) = if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(c) => (c, true),
            Err(e) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        serde_json::json!({ "error": format!("Failed to read config file: {e}") }),
                    ),
                )
                    .into_response();
            }
        }
    } else {
        (String::new(), false)
    };

    Json(ConfigResponse {
        path: path.to_string_lossy().to_string(),
        content,
        exists,
    })
    .into_response()
}

async fn update_config_handler(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> Response {
    // Validate TOML syntax
    if let Err(e) = req.content.parse::<toml_edit::DocumentMut>() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid TOML: {e}") })),
        )
            .into_response();
    }

    if let Some(parent) = state.config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::write(&state.config_path, &req.content) {
        Ok(_) => Json(serde_json::json!({
            "success": true,
            "path": state.config_path.to_string_lossy().to_string()
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
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
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "job_id": job_id })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn get_events_handler(State(state): State<AppState>, Path(job_id): Path<String>) -> Response {
    let sub_opt = state.runner.subscribe(&job_id).await;
    match sub_opt {
        Some((history, rx)) => {
            let history_stream =
                futures_util::stream::iter(history).filter_map(|event| async move {
                    let json = serde_json::to_string(&event).ok()?;
                    Some(Ok::<Event, std::convert::Infallible>(
                        Event::default().data(json),
                    ))
                });

            let live_stream = BroadcastStream::new(rx).filter_map(|res| async move {
                match res {
                    Ok(event) => {
                        let json = serde_json::to_string(&event).ok()?;
                        Some(Ok::<Event, std::convert::Infallible>(
                            Event::default().data(json),
                        ))
                    }
                    Err(_) => None,
                }
            });

            let combined_stream = history_stream.chain(live_stream);
            Sse::new(combined_stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
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
    if let Ok(Some(field)) = multipart.next_field().await {
        let file_name = field.file_name().unwrap_or("upload.tmp").to_string();
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Failed to read upload: {e}") })),
                )
                    .into_response();
            }
        };

        let temp_dir = std::env::temp_dir().join("lm_utils_uploads");
        let _ = std::fs::create_dir_all(&temp_dir);
        let dest = temp_dir.join(format!("{}_{}", uuid_timestamp(), file_name));
        if let Err(e) = std::fs::write(&dest, bytes) {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to save file: {e}") })),
            )
                .into_response();
        }

        return (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "path": dest.to_string_lossy().to_string() })),
        )
            .into_response();
    }

    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": "No file uploaded" })),
    )
        .into_response()
}

fn uuid_timestamp() -> String {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}_{}", now.as_secs(), now.subsec_nanos())
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

    let app = create_router(state);
    let addr: SocketAddr = format!("{host}:{port}").parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{local_addr}");

    anstream::println! {
        "{}\n🚀 Lunch Money Utilities Web GUI running at: {}\n{}",
        lm_common::style::STYLE_HEADER,
        url,
        lm_common::style::STYLE_HEADER
    };

    if open_browser {
        let _ = open::that(&url);
    }

    axum::serve(listener, app).await?;
    Ok(())
}
