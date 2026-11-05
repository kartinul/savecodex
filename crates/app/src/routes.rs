//! API routes: start solve / pack jobs, follow their events, fetch their screenshots and documents.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as UrlPath, Query},
    http::{header, HeaderMap, StatusCode},
    response::{sse::{Event as SseEvent, KeepAlive, Sse}, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use futures_util::{stream, StreamExt};
use serde_json::json;

use crate::ai::AiConfig;
use crate::jobs::{Event, Item, Job};
use crate::term_gen::{TermGenOptions, WindowStyle};

const PREVIEW: &str = "$ gcc fact.c -o fact && ./fact\nEnter a number: 5\nFactorial of 5 is 120\n";
const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

pub fn api_router() -> Router {
    Router::new()
        .route("/solve", post(|m: Multipart| start(Mode::Solve, m)))
        .route("/pack", post(|m: Multipart| start(Mode::Pack, m)))
        .route("/jobs/:id", delete(clear))
        .route("/jobs/:id/events", get(events))
        .route("/jobs/:id/shot/:n", get(shot))
        .route("/jobs/:id/doc/:n", get(doc))
        .route("/preview", get(preview))
        .route("/themes", get(|| async { Json(crate::doc_theme::DocTheme::presets()) }))
        .layer(DefaultBodyLimit::max(64 << 20))
}

#[derive(Clone, Copy)]
enum Mode { Solve, Pack }

type Fields = HashMap<String, String>;

fn bad(msg: impl std::fmt::Display) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg.to_string() }))).into_response()
}

/// POST /api/solve, /api/pack — multipart files (field name = relative path) and settings; returns the job id.
async fn start(mode: Mode, multipart: Multipart) -> Response {
    let (id, job) = match Job::create() {
        Ok(j) => j,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    };
    let input = job.dir.path().join("in");
    let (fields, files) = match read_form(multipart, &input).await {
        Ok(form) => form,
        Err(e) => { Job::remove(&id); return bad(e) }
    };
    if files.is_empty() {
        Job::remove(&id);
        return bad("no files uploaded");
    }
    let ai = match user_ai(&fields).await {
        Ok(ai) => ai,
        Err(e) => { Job::remove(&id); return bad(e) }
    };
    if let Err(e) = crate::runner::isolate_job_dir(job.dir.path()) {
        tracing::error!("Failed to isolate job dir: {e}");
        Job::remove(&id);
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "job isolation failure" }))).into_response();
    }
    let instructions = matches!(mode, Mode::Solve).then(|| field(&fields, "instructions").map(|t| t.chars().take(4000).collect())).flatten();
    let run = {
        let job = job.clone();
        async move {
            match mode {
                Mode::Solve => solve_job(&job, files, &fields).await,
                Mode::Pack => pack_job(&job, &input, files, &fields).await,
            }
            job.finish();
        }
    };
    job.set_task(tokio::spawn(crate::ai::scoped(crate::ai::RunAi { config: ai, instructions }, run)).abort_handle());
    Json(json!({ "id": id })).into_response()
}

async fn read_form(mut multipart: Multipart, dir: &Path) -> Result<(Fields, Vec<PathBuf>), String> {
    let (mut fields, mut files) = (Fields::new(), Vec::new());
    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or_default().to_string();
        if field.file_name().is_none() {
            fields.insert(name, field.text().await.unwrap_or_default());
            continue;
        }
        let rel = PathBuf::from(&name);
        if rel.as_os_str().is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(format!("bad file path: {name}"));
        }
        let bytes = field.bytes().await.map_err(|e| e.to_string())?;
        let path = dir.join(&rel);
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())? }
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        files.push(rel);
    }
    Ok((fields, files))
}

fn field<'a>(f: &'a Fields, key: &str) -> Option<&'a str> {
    f.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

fn term_options(f: &Fields) -> TermGenOptions {
    let text = |key, default: &str| field(f, key).unwrap_or(default).chars().take(64).collect::<String>();
    let style = match field(f, "style") {
        Some("macos") => WindowStyle::Macos,
        Some("ubuntu") => WindowStyle::Ubuntu,
        _ => WindowStyle::Windows,
    };
    TermGenOptions {
        font_size: 18.0,
        theme: if field(f, "theme") == Some("light") { "light" } else { "dark" }.into(),
        prompt: "$ ".into(),
        no_prompt_highlight: false,
        padding: style.padding(),
        messy: field(f, "messy").and_then(|v| v.parse().ok()).unwrap_or(0u8).min(3),
        seed: field(f, "seed").and_then(|v| v.parse().ok()),
        banner: false,
        min_cols: field(f, "min_cols").and_then(|v| v.parse().ok()).unwrap_or(80u16).clamp(40, 200),
        no_title_bar: false,
        style,
        username: text("username", "user"),
        hostname: text("hostname", "laptop"),
        cwd: text("cwd", "lab"),
    }
}

/// A docx file name from the user's pattern, kept inside the job dir.
fn out_name(pattern: &str, name: &str) -> String {
    let out: String = pattern.replace("{}", name).chars().filter(|c| !"/\\:*?\"<>|".contains(*c) && !c.is_control()).collect();
    let out = out.trim().trim_end_matches(".docx").trim_start_matches('.').to_string();
    if out.is_empty() { format!("{name}_solved") } else { out }
}

/// The user's own OpenAI-compatible key, if given. The base URL must be https and resolve to public
/// addresses only, so it can't be pointed at the server's own network.
async fn user_ai(f: &Fields) -> Result<Option<AiConfig>, String> {
    let Some(key) = field(f, "api_key") else { return Ok(None) };
    let model = field(f, "model").ok_or("a model name is needed with your own key")?;
    let base = field(f, "base_url").unwrap_or("https://api.openai.com/v1");
    let url = reqwest::Url::parse(base).map_err(|_| "the base url isn't a valid url")?;
    if url.scheme() != "https" { return Err("the base url must start with https://".into()) }
    let host = url.host_str().ok_or("the base url has no host")?;
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(443))).await
        .map_err(|_| "the base url's host can't be found")?.map(|a| a.ip()).collect();
    if addrs.is_empty() || addrs.iter().any(|ip| !is_public(*ip)) {
        return Err("the base url must be a public address".into());
    }
    Ok(Some(AiConfig::openai(key.into(), base.trim_end_matches('/').into(), model.into())))
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast()
                || v4.is_multicast() || a == 0 || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public(IpAddr::V4(v4)),
            None => {
                let first = v6.segments()[0];
                !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80)
            }
        },
    }
}

fn stem(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("solved").to_string()
}

fn theme(f: &Fields) -> crate::doc_theme::DocTheme {
    crate::doc_theme::DocTheme::from_web(field(f, "doc_theme").unwrap_or_default())
}

/// One document per PDF/DOCX, sharing the other files (images, data), like the CLI.
async fn solve_job(job: &Arc<Job>, files: Vec<PathBuf>, f: &Fields) {
    let input = job.dir.path().join("in");
    let is_doc = |p: &PathBuf| p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["pdf", "docx"].contains(&e.to_lowercase().as_str()));
    let (docs, shared): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().map(|p| input.join(p)).partition(is_doc);
    let runs: Vec<Vec<PathBuf>> = if docs.len() > 1 {
        docs.into_iter().map(|d| [vec![d], shared.clone()].concat()).collect()
    } else {
        vec![docs.into_iter().chain(shared).collect()]
    };
    job.emitter("").send(Event::Docs { items: runs.iter().enumerate().map(|(k, r)| Item { id: k.to_string(), title: stem(&r[0]) }).collect() });
    let multi = runs.len() > 1;
    let sem = Arc::new(tokio::sync::Semaphore::new(15));
    let term = term_options(f);
    let mut set = tokio::task::JoinSet::new();
    for (k, run) in runs.into_iter().enumerate() {
        let name = stem(&run[0]);
        let ev = job.emitter(k.to_string());
        let mut term = term.clone();
        term.cwd = term.cwd.replace("{}", &name);
        let opts = crate::solve::SolveOptions {
            term,
            output: Some(job.dir.path().join(out_name(field(f, "output").unwrap_or("{}_solved"), &name))),
            doc_title: field(f, "doc_title").map(|t| t.replace("{}", &name)),
            doc_text: field(f, "doc_text").map(|t| t.replace("{}", &name)),
            page_break: field(f, "page_break") == Some("true"),
            theme: theme(f),
            trim: true,
            jobs: 15,
            questions: false,
            max_pages: 60,
            check: (field(f, "check") == Some("true")).then_some(0.7),
            sem: sem.clone(),
            tag: if multi { name.clone() } else { String::new() },
            events: Some(ev.clone()),
        };
        set.spawn(crate::ai::inherit(async move {
            if let Err(e) = crate::solve::run_solve(&run, opts).await {
                tracing::warn!("solve of {} failed: {e:#}", run[0].display());
                ev.send(Event::Error { message: format!("{e:#}") });
            }
        }));
    }
    set.join_all().await;
}

async fn pack_job(job: &Arc<Job>, input: &Path, files: Vec<PathBuf>, f: &Fields) {
    // a picked folder arrives as folder/…; loose files sit right in the input dir
    let top = files[0].components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
    let (folder, name) = if files.len() > 0 && input.join(&top).is_dir() { (input.join(&top), top) } else { (input.to_path_buf(), "code".into()) };
    job.emitter("").send(Event::Docs { items: vec![Item { id: "0".into(), title: name.clone() }] });
    let ev = job.emitter("0");
    let mut term = term_options(f);
    term.cwd = term.cwd.replace("{}", &name);
    let out = job.dir.path().join(out_name(field(f, "output").unwrap_or("{}_pack"), &name));
    let title = field(f, "doc_title").map(|t| t.replace("{}", &name)).unwrap_or_else(|| name.clone());
    let text = field(f, "doc_text").map(|t| t.replace("{}", &name));
    let page_break = field(f, "page_break") == Some("true");
    match crate::pack::run_pack(&folder, Some(&out), &[], Some(&title), text.as_deref(), page_break, &theme(f), &term, 15, true, crate::pack::AiInput::Auto, true, Some(ev.clone())).await {
        Ok(Some(path)) => ev.done(path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path),
        Ok(None) => ev.send(Event::Error { message: "no code files found".into() }),
        Err(e) => {
            tracing::warn!("pack of {name} failed: {e:#}");
            ev.send(Event::Error { message: format!("{e:#}") })
        }
    }
}

/// GET /api/jobs/:id/events — every event so far, then new ones as they come (SSE). Resumes after Last-Event-ID.
async fn events(UrlPath(id): UrlPath<String>, headers: HeaderMap) -> Response {
    let Some(job) = Job::get(&id) else { return StatusCode::NOT_FOUND.into_response() };
    let from = headers.get("last-event-id").and_then(|v| v.to_str().ok()?.parse::<usize>().ok()).map_or(0, |n| n + 1);
    let stream = stream::unfold((job, from), |(job, from)| async move {
        let batch = job.next(from).await?;
        let next = from + batch.len();
        let batch = batch.into_iter().enumerate().map(move |(i, e)| Ok::<_, Infallible>(SseEvent::default().id((from + i).to_string()).data(e)));
        Some((stream::iter(batch), (job, next)))
    }).flatten();
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

async fn shot(UrlPath((id, n)): UrlPath<(String, usize)>) -> Response {
    match Job::get(&id).and_then(|j| j.shot(n)) {
        Some(png) => ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "private, max-age=3600")], png).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn doc(UrlPath((id, n)): UrlPath<(String, usize)>) -> Response {
    let Some((name, path)) = Job::get(&id).and_then(|j| j.doc(n)) else { return StatusCode::NOT_FOUND.into_response() };
    match std::fs::read(&path) {
        Ok(bytes) => {
            let name: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' || c == ' ' { c } else { '_' }).collect();
            ([(header::CONTENT_TYPE, DOCX.to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], bytes).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// DELETE /api/jobs/:id — stops the run and forgets it.
async fn clear(UrlPath(id): UrlPath<String>) -> StatusCode {
    Job::remove(&id);
    StatusCode::NO_CONTENT
}

/// GET /api/preview — a sample screenshot with the given look.
async fn preview(Query(q): Query<Fields>) -> Response {
    let opts = term_options(&q);
    let png = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let img = crate::term_gen::generate_terminal_image(PREVIEW, &opts)?;
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageOutputFormat::Png)?;
        Ok(buf.into_inner())
    }).await;
    match png {
        Ok(Ok(png)) => ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=86400")], png).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses() {
        for ip in ["127.0.0.1", "10.1.2.3", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1"] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "2606:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn output_names_stay_in_the_job_dir() {
        assert_eq!(out_name("{}_solved", "DSA Lab 3"), "DSA Lab 3_solved");
        assert_eq!(out_name("../../etc/passwd", "x"), "etcpasswd");
        assert_eq!(out_name("report.docx", "x"), "report");
        assert_eq!(out_name("  ", "x"), "x_solved");
    }
}
