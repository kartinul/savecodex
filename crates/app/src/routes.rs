//! API routes — `/api/pack`.

use std::path::Path;
use axum::{
    extract::Multipart,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::json;

pub fn api_router() -> Router {
    Router::new()
        .route("/pack", post(pack))
        .route("/verify_admin", post(verify_admin))
}



/// POST /api/pack — multipart upload of source files (field name = relative path).
async fn pack(mut multipart: Multipart) -> impl IntoResponse {
    tracing::debug!("Received /api/pack request, starting multipart parsing");
    let temp_dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => {
            tracing::error!("Failed to create temp dir: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "temp dir failure" }))).into_response();
        }
    };

    let mut ext = String::new();
    let mut doc_title = String::new();
    let mut doc_text = String::new();
    let mut font_size: f32 = 18.0;
    let mut theme = "dark".to_string();
    let mut prompt = "$ ".to_string();
    let mut no_prompt_highlight = false;
    let mut padding: i32 = 28;
    let mut style = crate::term_gen::WindowStyle::Windows;
    let mut username = "local".to_string();
    let mut hostname = "host".to_string();
    let mut cwd = "~".to_string();
    let mut page_break = false;
    let mut folder_name = "folder".to_string();
    let mut got_files = false;

    let mut gemini_api_key = String::new();
    let mut admin_password = String::new();
    let mut ai_provider = String::new();
    let mut ai_model = String::new();
    let mut ai_base_url = String::new();

    tracing::debug!("Extracting multipart fields...");
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let field_name = field.name().unwrap_or("unknown").to_string();
        let file_name = field.file_name().map(|s| s.to_string());
        
        tracing::debug!("Processing field: {}, file_name: {:?}", field_name, file_name);

        if file_name.is_some() {
            // It's a file. Reconstruct the structure in temp dir using field_name (webkitRelativePath)
            let bytes = match field.bytes().await {
                Ok(b) => b,
                Err(_) => continue,
            };

            // Use field_name which has the full relative path like folder/src/main.rs
            let path = Path::new(&field_name);
            if let Some(first_comp) = path.components().next() {
                let fname = first_comp.as_os_str().to_string_lossy().to_string();
                if folder_name == "folder" {
                    folder_name = fname;
                }
            }

            let full_path = temp_dir.path().join(path);
            if let Some(p) = full_path.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if let Err(e) = std::fs::write(&full_path, bytes) {
                tracing::error!("Failed to write file {}: {}", field_name, e);
            }
            got_files = true;
        } else {
            // It's a parameter.
            let text = field.text().await.unwrap_or_default();
            match field_name.as_str() {
                "ext" => ext = text,
                "doc_title" => doc_title = text,
                "doc_text" => doc_text = text,
                "font_size" => font_size = text.parse().unwrap_or(18.0),
                "theme" => theme = text,
                "prompt" => prompt = text,
                "no_prompt_highlight" => no_prompt_highlight = text == "true",
                "padding" => padding = text.parse().unwrap_or(28),
                "style" => {
                    style = match text.as_str() {
                        "macos" => crate::term_gen::WindowStyle::Macos,
                        "linux" => crate::term_gen::WindowStyle::Linux,
                        _ => crate::term_gen::WindowStyle::Windows,
                    };
                }
                "username" => username = text,
                "hostname" => hostname = text,
                "cwd" => cwd = text,
                "page_break" => page_break = text == "true",
                "gemini_api_key" => gemini_api_key = text,
                "admin_password" => admin_password = text,
                "ai_provider" => ai_provider = text,
                "ai_model" => ai_model = text,
                "ai_base_url" => ai_base_url = text,
                _ => {}
            }
        }
    }

    tracing::debug!("Finished extracting multipart fields. Files received: {}", got_files);

    let env_admin_pwd = std::env::var("ADMIN_PASSWORD").unwrap_or_default();
    let mut final_api_key = gemini_api_key.clone();

    if !admin_password.is_empty() && admin_password == env_admin_pwd {
        final_api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
    } else if final_api_key.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "API Key is required" }))).into_response();
    }

    if !final_api_key.is_empty() {
        std::env::set_var("GEMINI_API_KEY", final_api_key.clone());
        std::env::set_var("INPUT_API_KEY", final_api_key);
    }
    if !ai_provider.is_empty() {
        std::env::set_var("INPUT_PROVIDER", ai_provider);
    }
    if !ai_model.is_empty() {
        std::env::set_var("INPUT_MODEL", ai_model);
    }
    if !ai_base_url.is_empty() {
        std::env::set_var("INPUT_BASE_URL", ai_base_url);
    }

    if !got_files {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "no files uploaded" }))).into_response();
    }

    let resolved_cwd = cwd.replace("{}", &folder_name);

    let opts = crate::term_gen::TermGenOptions {
        font_size,
        theme,
        prompt,
        no_prompt_highlight,
        padding,
        style,
        username,
        hostname,
        cwd: resolved_cwd,
    };

    let ext_list: Vec<String> = if ext.is_empty() {
        Vec::new()
    } else {
        ext.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    };

    let resolved_doc_title = doc_title.replace("{}", &folder_name);
    let resolved_doc_text = doc_text.replace("{}", &folder_name);

    let doc_title_opt = if doc_title.is_empty() { None } else { Some(resolved_doc_title.as_str()) };
    let doc_text_opt = if doc_text.is_empty() { None } else { Some(resolved_doc_text.as_str()) };

    let target_folder = temp_dir.path().join(&folder_name);
    let pack_folder = if target_folder.is_dir() {
        target_folder.as_path()
    } else {
        temp_dir.path()
    };

    let output_path = temp_dir.path().join(format!("{}_pack", folder_name));

    tracing::info!("Running pack on {}", pack_folder.display());
    match crate::pack::run_pack(
        pack_folder,
        Some(&output_path),
        &ext_list,
        doc_title_opt,
        doc_text_opt,
        page_break,
        &opts,
    ).await {
        Ok(_) => {
            let out_docx = output_path.with_extension("docx");
            match std::fs::read(&out_docx) {
                Ok(bytes) => {
                    let filename = format!("{}_pack.docx", folder_name);
                    (
                        StatusCode::OK,
                        [
                            (header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
                            (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{}\"", filename)),
                        ],
                        bytes
                    ).into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to read generated docx: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "failed to read output file" }))).into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("run_pack failed: {}", e);
            let status = if e.to_string().contains("Invalid API Key") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({ "error": e.to_string() }))).into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct VerifyAdminPayload {
    admin_password: Option<String>,
}

async fn verify_admin(Json(payload): Json<VerifyAdminPayload>) -> impl IntoResponse {
    tracing::debug!("Received /api/verify_admin request");
    let env_admin_pwd = std::env::var("ADMIN_PASSWORD").unwrap_or_default();
    let sent_pwd = payload.admin_password.unwrap_or_default();

    if !sent_pwd.is_empty() && sent_pwd == env_admin_pwd {
        (StatusCode::OK, Json(json!({ "success": true }))).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "success": false, "error": "Invalid admin password" }))).into_response()
    }
}
