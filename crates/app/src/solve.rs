use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::ai::{ask_code, ask_code_json, Part};
use crate::docx_gen::PackItem;
use crate::pack::AiInput;
use crate::prompts::{after_run_prompt, extract_prompt, fix_diagram_prompt, solve_prompt};
use crate::runner::{load_configurations, LangConfig};
use crate::term_gen::TermGenOptions;

#[derive(Deserialize)]
struct Sheet {
    groups: Vec<Group>,
}

#[derive(Deserialize, Serialize)]
struct Group {
    #[serde(default)]
    setup: Option<Setup>,
    questions: Vec<Question>,
}

#[derive(Deserialize, Serialize)]
struct Setup {
    #[serde(default)]
    lang: String,
    #[serde(default)]
    code: String,
}

#[derive(Deserialize, Serialize)]
struct Question {
    title: String,
    text: String,
    #[serde(default)]
    parts: Vec<String>,
}

#[derive(Deserialize)]
struct Solved {
    title: String,
    answers: Vec<Answer>,
}

#[derive(Deserialize)]
struct Answer {
    label: String,
    lang: Option<String>,
    #[serde(default)]
    code: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    answer_after_run: bool,
}

pub struct SolveOptions {
    pub term: TermGenOptions,
    pub output: Option<PathBuf>,
    pub doc_title: Option<String>,
    pub doc_text: Option<String>,
    pub page_break: bool,
    pub ai_input: AiInput,
    pub trim: bool,
    pub jobs: usize,
    pub questions: bool,
    pub max_pages: usize,
}

/// Everything read from the input files.
#[derive(Default)]
struct Inputs {
    text: String,
    images: Vec<Part>,
    data_files: Vec<PathBuf>,
}

/// Outcome of one answer, for the summary line.
enum Status {
    Done,
    NoScreenshot,
    Failed,
}

pub async fn run_solve(inputs: &[PathBuf], opts: SolveOptions) -> Result<()> {
    let stem = inputs[0].file_stem().and_then(|s| s.to_str()).unwrap_or("solved").to_string();
    let read = read_inputs(inputs, opts.max_pages)?;
    let configs = load_configurations()?;

    let model = crate::ai::AiConfig::from_env("CODE")?.model;
    println!("🔍 Extracting questions with {} ({} image(s))…", model, read.images.len() / 2);
    let mut parts = vec![Part::Text(extract_prompt(&read.text, &lang_names(&configs, |c| c.session)))];
    parts.extend(read.images);
    let mut sheet: Sheet = ask_json(&parts).await.context("extracting questions")?;
    for group in &mut sheet.groups {
        group.setup = group.setup.take().filter(|s| !s.code.trim().is_empty());
    }
    let total: usize = sheet.groups.iter().map(|g| g.questions.len()).sum();
    println!("📝 {} question(s) in {} group(s) — solving up to {} at a time", total, sheet.groups.len(), opts.jobs);

    let work = tempfile::tempdir()?;
    let langs = Arc::new(describe_langs(&configs));
    let configs = Arc::new(configs);
    let data_files = Arc::new(read.data_files);
    let opts = Arc::new(opts);
    let sem = Arc::new(Semaphore::new(opts.jobs.max(1)));
    let mut set = JoinSet::new();
    for (n, group) in sheet.groups.into_iter().enumerate() {
        let dir = work.path().join(format!("g{}", n + 1));
        let (configs, langs, data_files, opts, sem) = (configs.clone(), langs.clone(), data_files.clone(), opts.clone(), sem.clone());
        set.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let titles = group.questions.iter().map(|q| q.title.clone()).collect::<Vec<_>>().join(", ");
            let result = solve_group(n + 1, &group, &dir, &configs, &langs, &data_files, &opts).await;
            (n, result.unwrap_or_else(|e| {
                tracing::warn!("group {} failed: {:#}", n + 1, e);
                let item = PackItem { heading: Some(titles.clone()), answer: format!("[solve failed: {e:#}]"), ..Default::default() };
                vec![(item, Status::Failed, titles)]
            }))
        });
    }
    let mut results: Vec<_> = set.join_all().await;
    results.sort_by_key(|(n, _)| *n);

    let mut items = Vec::new();
    let (mut done, mut failed, mut no_shot) = (0, Vec::new(), Vec::new());
    for (item, status, label) in results.into_iter().flat_map(|(_, r)| r) {
        match status {
            Status::Done => done += 1,
            Status::NoScreenshot => no_shot.push(label),
            Status::Failed => failed.push(label),
        }
        items.push(item);
    }

    let out = opts.output.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_solved"))).with_extension("docx");
    let title = opts.doc_title.clone().unwrap_or(stem);
    crate::docx_gen::generate_docx(&out, &title, opts.doc_text.as_deref(), &items, opts.page_break)?;
    println!("Done → {}", out.display());
    println!("{} done, {} failed{}, {} without output{}", done,
        failed.len(), list(&failed), no_shot.len(), list(&no_shot));
    Ok(())
}

fn list(labels: &[String]) -> String {
    if labels.is_empty() { String::new() } else { format!(" ({})", labels.join(", ")) }
}

async fn solve_group(
    n: usize,
    group: &Group,
    dir: &Path,
    configs: &HashMap<String, LangConfig>,
    langs: &str,
    data_files: &[PathBuf],
    opts: &SolveOptions,
) -> Result<Vec<(PackItem, Status, String)>> {
    std::fs::create_dir_all(dir)?;
    for file in data_files {
        std::fs::copy(file, dir.join(file.file_name().context("bad data file name")?))?;
    }
    let solved: Vec<Solved> = ask_json(&[Part::Text(solve_prompt(&serde_json::to_string_pretty(group)?, langs))]).await
        .context("solving")?;

    let session_langs: HashSet<&str> = group.setup.iter().map(|s| s.lang.as_str())
        .chain(solved.iter().flat_map(|q| q.answers.iter().filter_map(|a| a.lang.as_deref())))
        .filter(|l| configs.get(*l).is_some_and(|c| c.session))
        .collect();
    for lang in &session_langs {
        if let Some(reset) = &configs[*lang].reset {
            if let Err(e) = crate::runner::run_piped(&crate::runner::with_group(reset, n), "", dir).await {
                tracing::warn!("[group {n}] {lang} reset failed: {e:#}");
            }
        }
    }

    let mut items = Vec::new();
    if let Some(setup) = &group.setup {
        match configs.get(&setup.lang).filter(|c| c.session) {
            Some(cfg) => if let Err(e) = crate::runner::run_piped(&crate::runner::with_group(&cfg.run, n), &setup.code, dir).await {
                tracing::warn!("[group {n}] setup failed: {e:#}");
            },
            None => tracing::warn!("[group {n}] setup language {} is not a session language, skipped", setup.lang),
        }
        let item = PackItem { heading: Some("Setup".into()), content: setup.code.clone(), ..Default::default() };
        items.push((item, Status::Done, "Setup".into()));
    }

    let answers: Vec<(&Solved, usize, &Answer)> = solved.iter()
        .flat_map(|q| q.answers.iter().enumerate().map(move |(i, a)| (q, i, a)))
        .collect();
    let transcripts = run_sessions(n, &answers, configs, dir).await;
    let mut names = HashSet::new();

    for (idx, (q, i, a)) in answers.iter().enumerate() {
        let question = group.questions.iter().find(|x| x.title == q.title);
        let mut term = opts.term.clone();
        term.no_title_bar = *i > 0;
        let mut item = PackItem {
            heading: Some(a.label.clone()),
            content: a.code.clone(),
            answer: a.answer.clone(),
            question: question.filter(|_| opts.questions && *i == 0).map(question_text).unwrap_or_default(),
            ..Default::default()
        };
        let mut status = Status::Done;
        let output = match a.lang.as_deref() {
            None => None,
            Some(lang) => match configs.get(lang) {
                None => {
                    tracing::warn!("[{}] language {lang} isn't configured, no screenshot", a.label);
                    status = Status::NoScreenshot;
                    None
                }
                Some(cfg) if cfg.image => {
                    match render_diagram(cfg, dir, &unique_name(&a.label, lang, &mut names), &a.code).await {
                        Ok(png) => {
                            let img = image::load_from_memory(&png)?;
                            (item.img_width, item.img_height) = (img.width(), img.height());
                            item.img_bytes = png;
                            item.content.clear();
                        }
                        Err(e) => {
                            item.answer = format!("{}\n[diagram failed to render: {e:#}]", item.answer).trim().to_string();
                            status = Status::NoScreenshot;
                        }
                    }
                    None
                }
                Some(cfg) if cfg.session => {
                    let out = transcripts.get(&idx).cloned().unwrap_or_default();
                    (item.img_bytes, item.img_width, item.img_height) = crate::pack::screenshot(None, &out, &term, opts.trim)?;
                    Some(out)
                }
                Some(_) => {
                    let filename = unique_name(&a.label, lang, &mut names);
                    let path = dir.join(&filename);
                    std::fs::write(&path, &a.code)?;
                    let (cmd, out) = crate::pack::run_with_input(&path, dir, &filename, &a.code, None, opts.ai_input).await;
                    (item.img_bytes, item.img_width, item.img_height) = crate::pack::screenshot(Some(&cmd), &out, &term, opts.trim)?;
                    Some(out)
                }
            },
        };
        if let (true, Some(out)) = (a.answer_after_run, &output) {
            let q_text = question.map(question_text).unwrap_or_default();
            match ask_code(&[Part::Text(after_run_prompt(&q_text, &a.code, out))]).await {
                Ok(text) => item.answer = text.trim().to_string(),
                Err(e) => tracing::warn!("[{}] written answer failed: {e:#}", a.label),
            }
        }
        if item.content.is_empty() && item.answer.is_empty() && item.img_bytes.is_empty() {
            status = Status::Failed;
        }
        items.push((item, status, a.label.clone()));
    }
    Ok(items)
}

/// Runs each stretch of consecutive answers in the same session language as one session,
/// so state (tables, variables, cwd) carries over. Returns transcripts by answer index.
async fn run_sessions(n: usize, answers: &[(&Solved, usize, &Answer)], configs: &HashMap<String, LangConfig>, dir: &Path) -> HashMap<usize, String> {
    let session_of = |a: &Answer| a.lang.as_deref().filter(|l| configs.get(*l).is_some_and(|c| c.session)).map(str::to_string);
    let mut transcripts = HashMap::new();
    let mut idx = 0;
    while idx < answers.len() {
        let Some(lang) = session_of(answers[idx].2) else { idx += 1; continue };
        let batch: Vec<usize> = (idx..answers.len()).take_while(|&j| session_of(answers[j].2).as_deref() == Some(lang.as_str())).collect();
        idx += batch.len();
        let cfg = &configs[&lang];
        let (argv, prompts, dir) = (crate::runner::with_group(&cfg.run, n), cfg.prompts.clone(), dir.to_path_buf());
        let chunks: Vec<String> = batch.iter().map(|&j| answers[j].2.code.clone()).collect();
        let result = tokio::task::spawn_blocking(move || crate::runner::run_session(&argv, &prompts, &chunks, &dir)).await;
        match result {
            Ok(Ok(outs)) => transcripts.extend(batch.into_iter().zip(outs)),
            Ok(Err(e)) => transcripts.extend(batch.into_iter().map(|j| (j, format!("[Error starting {lang}: {e:#}]")))),
            Err(e) => transcripts.extend(batch.into_iter().map(|j| (j, format!("[Error: {e}]")))),
        }
    }
    transcripts
}

/// Render a diagram; on a syntax error ask the model to fix it once.
async fn render_diagram(cfg: &LangConfig, dir: &Path, filename: &str, code: &str) -> Result<Vec<u8>> {
    let path = dir.join(filename);
    std::fs::write(&path, code)?;
    let err = match crate::runner::render_image(cfg, &path, dir).await {
        Ok(png) => return Ok(png),
        Err(e) => e,
    };
    tracing::warn!("[{filename}] diagram failed, asking for a fix: {err:#}");
    let fixed = strip_fences(&ask_code(&[Part::Text(fix_diagram_prompt(code, &format!("{err:#}")))]).await?).to_string();
    std::fs::write(&path, &fixed)?;
    crate::runner::render_image(cfg, &path, dir).await
}

fn question_text(q: &Question) -> String {
    std::iter::once(q.text.as_str()).chain(q.parts.iter().map(String::as_str)).collect::<Vec<_>>().join("\n")
}

/// `Q3(b)` → `q3b.<ext>`, unique within the group.
fn unique_name(label: &str, ext: &str, taken: &mut HashSet<String>) -> String {
    let slug: String = label.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect();
    let slug = if slug.starts_with(|c: char| c.is_ascii_alphabetic()) { slug } else { format!("q{slug}") };
    let name = (1..).map(|i| if i == 1 { format!("{slug}.{ext}") } else { format!("{slug}_{i}.{ext}") })
        .find(|n| !taken.contains(n)).unwrap();
    taken.insert(name.clone());
    name
}

fn lang_names(configs: &HashMap<String, LangConfig>, pick: impl Fn(&LangConfig) -> bool) -> String {
    let mut names: Vec<&str> = configs.iter().filter(|(_, c)| pick(c)).map(|(k, _)| k.as_str()).collect();
    names.sort();
    names.join(", ")
}

fn describe_langs(configs: &HashMap<String, LangConfig>) -> String {
    format!(
        "- file (saved as <label>.<lang> and run; Java runs as one file with `java file.java`, so declare the class with main first): {}\n\
         - session (typed into an interactive client line by line; state persists across the group): {}\n\
         - image (rendered to a picture, code not shown): {}",
        lang_names(configs, |c| !c.session && !c.image),
        lang_names(configs, |c| c.session),
        lang_names(configs, |c| c.image),
    )
}

fn strip_fences(text: &str) -> &str {
    let t = text.trim();
    let t = t.strip_prefix("```").map_or(t, |rest| rest.split_once('\n').map_or(rest, |(_, body)| body));
    t.strip_suffix("```").unwrap_or(t).trim()
}

/// Ask for JSON, retrying once when the reply doesn't parse.
async fn ask_json<T: DeserializeOwned>(parts: &[Part]) -> Result<T> {
    match parse_json(&ask_code_json(parts).await?) {
        Ok(value) => Ok(value),
        Err(e) => {
            tracing::warn!("bad JSON from model, retrying: {e:#}");
            parse_json(&ask_code_json(parts).await?)
        }
    }
}

/// Parse the JSON object or array in a model reply, ignoring markdown fences or chatter around it.
fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T> {
    let start = text.find(['{', '[']).context("no JSON in reply")?;
    let end = text.rfind(['}', ']']).context("no JSON in reply")?;
    serde_json::from_str(&text[start..=end]).with_context(|| format!("bad JSON: {}", &text[..text.len().min(300)]))
}

fn read_inputs(paths: &[PathBuf], max_pages: usize) -> Result<Inputs> {
    let mut read = Inputs::default();
    for path in paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("input").to_string();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let (text, images) = match ext.as_str() {
            "pdf" => read_pdf(path, max_pages)?,
            "docx" => read_docx(path)?,
            "png" | "jpg" | "jpeg" | "webp" => (String::new(), vec![(mime(&ext), std::fs::read(path)?)]),
            _ => {
                read.data_files.push(path.clone());
                (std::fs::read_to_string(path).with_context(|| format!("can't read {name} as text"))?, vec![])
            }
        };
        read.text.push_str(&format!("--- {name} ---\n{text}\n\n"));
        for (i, (mime, bytes)) in images.into_iter().enumerate() {
            read.images.push(Part::Text(format!("{name}, image {}:", i + 1)));
            read.images.push(Part::Image(mime, bytes));
        }
    }
    Ok(read)
}

fn mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

fn tool(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd).args(args).output()
        .with_context(|| format!("{cmd} not found (install poppler)"))?;
    if !out.status.success() {
        bail!("{cmd} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn read_pdf(path: &Path, max_pages: usize) -> Result<(String, Vec<(&'static str, Vec<u8>)>)> {
    let file = path.to_str().context("non-UTF-8 path")?;
    let info = tool("pdfinfo", &[file]).with_context(|| format!("can't open {file} (password-protected PDFs aren't supported)"))?;
    let pages: usize = info.lines().find_map(|l| l.strip_prefix("Pages:")).and_then(|p| p.trim().parse().ok()).unwrap_or(0);
    if pages > max_pages {
        bail!("{file} has {pages} pages (limit {max_pages}, raise with --max-pages)");
    }
    let text = tool("pdftotext", &["-layout", file, "-"])?;
    let pics = tempfile::tempdir()?;
    let prefix = pics.path().join("page");
    tool("pdftoppm", &["-r", "110", "-jpeg", file, prefix.to_str().context("non-UTF-8 temp path")?])?;
    let mut pages: Vec<PathBuf> = std::fs::read_dir(pics.path())?.filter_map(|e| Some(e.ok()?.path())).collect();
    pages.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));
    let images = pages.iter().map(|p| Ok(("image/jpeg", std::fs::read(p)?))).collect::<Result<_>>()?;
    Ok((text, images))
}

fn read_docx(path: &Path) -> Result<(String, Vec<(&'static str, Vec<u8>)>)> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?).context("not a valid .docx")?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")?.read_to_string(&mut xml)?;
    let mut images = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let ext = entry.name().rsplit('.').next().unwrap_or("").to_lowercase();
        if entry.name().starts_with("word/media/") && matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            images.push((mime(&ext), bytes));
        }
    }
    Ok((docx_text(&xml), images))
}

/// Plain text of a DOCX body: one line per paragraph, tabs kept, tags dropped.
fn docx_text(xml: &str) -> String {
    let xml = xml.replace("</w:p>", "\n").replace("<w:tab/>", "\t").replace("<w:br/>", "\n");
    let mut text = String::new();
    let mut in_tag = false;
    for c in xml.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json() {
        let solved: Vec<Solved> = parse_json("Sure!\n```json\n[{\"title\":\"Q1\",\"answers\":[{\"label\":\"Q1\",\"lang\":null}]}]\n```").unwrap();
        assert_eq!(solved[0].answers[0].label, "Q1");
        assert!(solved[0].answers[0].lang.is_none());
        assert_eq!(strip_fences("```dot\ndigraph{}\n```"), "digraph{}");
    }

    #[test]
    fn names_are_unique_slugs() {
        let mut taken = HashSet::new();
        assert_eq!(unique_name("Q3(b)", "cpp", &mut taken), "q3b.cpp");
        assert_eq!(unique_name("Q3 (b)", "cpp", &mut taken), "q3b_2.cpp");
        assert_eq!(unique_name("1.", "sh", &mut taken), "q1.sh");
    }

    #[test]
    fn docx_text_keeps_paragraphs() {
        assert_eq!(docx_text("<w:p><w:t>a &amp; b</w:t></w:p><w:p><w:t>c</w:t><w:tab/><w:t>d</w:t></w:p>"), "a & b\nc\td\n");
    }
}
