//! API routes — `/api/pack` and `/api/solve`.

use axum::{
    extract::Multipart,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::json;

pub fn api_router() -> Router {
    Router::new()
        .route("/pack",  post(pack))
        .route("/solve", post(solve))
}

/// POST /api/pack — multipart upload of source files (field name = relative path).
async fn pack(mut multipart: Multipart) -> impl IntoResponse {
    let mut files: Vec<String> = Vec::new();

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("unknown").to_string();
        let data = match field.bytes().await {
            Ok(b) => b,
            Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "bad upload" }))).into_response(),
        };
        tracing::info!("pack: received file {} ({} bytes)", name, data.len());
        files.push(name);
    }

    // TODO: run each file, capture output screenshot, assemble DOCX + PDF.
    (StatusCode::OK, Json(json!({ "status": "stub", "files": files }))).into_response()
}

/// POST /api/solve — single assignment file (.pdf or .docx).
async fn solve(mut multipart: Multipart) -> impl IntoResponse {
    let field = match multipart.next_field().await {
        Ok(Some(f)) => f,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "no file" }))).into_response(),
    };

    let filename = field.file_name().unwrap_or("assignment").to_string();
    let data = match field.bytes().await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "bad upload" }))).into_response(),
    };

    tracing::info!("solve: received {} ({} bytes)", filename, data.len());

    // TODO: parse questions, call AI, run generated code, assemble DOCX + PDF.
    (StatusCode::OK, Json(json!({ "status": "stub", "file": filename }))).into_response()
}
