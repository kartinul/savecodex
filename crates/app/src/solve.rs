use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::ai::{ask, Part, Tier};
use crate::docx_gen::{PackItem, Shot};
use crate::jobs::{Emitter, Event, Item};
use crate::pack::AiInput;
use crate::prompts::{after_run_prompt, check_prompt, extract_prompt, fix_prompt, review_prompt, solve_prompt};
use crate::runner::{final_screen, full_screen, load_configurations, LangConfig};
use crate::term_gen::TermGenOptions;

#[derive(Deserialize)]
struct Extracted {
    #[serde(default)]
    language: String,
    groups: Vec<Group>,
}

#[derive(Deserialize, Serialize)]
struct Group {
    #[serde(default, deserialize_with = "lenient")]
    setup: Option<Setup>,
    questions: Vec<Question>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Setup {
    #[serde(default)]
    lang: String,
    #[serde(default)]
    code: String,
}

#[derive(Deserialize, Serialize)]
struct Question {
    title: String,
    /// Set by `assign_labels`, one style for the whole document.
    #[serde(default)]
    label: String,
    text: String,
    #[serde(default)]
    parts: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Solved {
    title: String,
    answers: Vec<Answer>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Answer {
    label: String,
    lang: Option<String>,
    #[serde(default)]
    code: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    answer_after_run: bool,
    #[serde(default)]
    files: Vec<NamedFile>,
    #[serde(default)]
    stdin: String,
    #[serde(default)]
    args: Vec<String>,
    /// The error is what the question wants shown, so it isn't sent back for a fix.
    #[serde(default)]
    expects_error: bool,
    /// Shown above the answer, set by `prepare`.
    #[serde(skip)]
    heading: String,
    /// The heading with every part kept, which names the answer's file.
    #[serde(skip)]
    slug: String,
    /// The part positions shown as sub-headings in the document, empty when the heading has no parts.
    #[serde(skip)]
    parts: Vec<usize>,
}

#[derive(Clone, Deserialize, Serialize)]
struct NamedFile {
    name: String,
    code: String,
}

pub struct SolveOptions {
    pub term: TermGenOptions,
    pub output: Option<PathBuf>,
    pub doc_title: Option<String>,
    pub doc_text: Option<String>,
    pub page_break: bool,
    pub theme: crate::doc_theme::DocTheme,
    pub trim: bool,
    pub jobs: usize,
    pub questions: bool,
    pub max_pages: usize,
    /// `--check`: the confidence a SMART "bad" grade needs to redo an answer; None skips grading.
    pub check: Option<f32>,
    /// Shared by every document solved at once, so `jobs` caps them together.
    pub sem: Arc<Semaphore>,
    /// Prefix for progress lines when several documents run at once.
    pub tag: String,
    /// Live progress for the web UI.
    pub events: Option<Emitter>,
}

const QUESTIONS_PER_CALL: usize = 4;
/// Attempts at fixing an answer that crashes, fails or doesn't render.
const FIXES: usize = 3;

/// How much help the models needed across the whole run, and what `--check` found.
struct Stats {
    answers: AtomicUsize,
    bad_json: AtomicUsize,
    asked_again: AtomicUsize,
    fixes: AtomicUsize,
    redone: AtomicUsize,
    good: AtomicUsize,
    bad: AtomicUsize,
}

static STATS: Stats = Stats {
    answers: AtomicUsize::new(0),
    bad_json: AtomicUsize::new(0),
    asked_again: AtomicUsize::new(0),
    fixes: AtomicUsize::new(0),
    redone: AtomicUsize::new(0),
    good: AtomicUsize::new(0),
    bad: AtomicUsize::new(0),
};

fn count(stat: &AtomicUsize, n: usize) {
    stat.fetch_add(n, Ordering::Relaxed);
}

/// Prints what `--check` found and warns when the models needed a lot of help.
pub fn print_stats() {
    let get = |stat: &AtomicUsize| stat.load(Ordering::Relaxed);
    let s = &STATS;
    let (answers, bad_json, asked_again, fixes, redone) = (get(&s.answers), get(&s.bad_json), get(&s.asked_again), get(&s.fixes), get(&s.redone));
    let answered = crate::ai::answered_by();
    if !answered.is_empty() {
        println!("🤖 answered by: {}", answered.iter().map(|(m, n)| format!("{m} ×{n}")).collect::<Vec<_>>().join(", "));
    }
    if get(&s.good) + get(&s.bad) > 0 {
        println!("🔎 check: {} good, {} bad, {redone} question(s) redone", get(&s.good), get(&s.bad));
    }
    // shortcut: help on over a quarter of the answers counts as weak; tune if it warns on good models
    if (bad_json + asked_again + fixes + redone) * 4 > answers.max(1) {
        println!("⚠️  {} looks weak here: {bad_json} bad JSON replies, {asked_again} re-asks, {fixes} fixes and {redone} redos over {answers} answers. A stronger SMART/MID model would do better.",
            crate::ai::describe(Tier::Mid));
    }
}

/// Session namespaces (`{ns}`) must not collide across documents solved at once.
static NEXT_SESSION_ID: AtomicUsize = AtomicUsize::new(1);
/// Random per process, so namespaces don't collide with other runs or anything already there.
static RUN_ID: std::sync::LazyLock<u32> = std::sync::LazyLock::new(|| {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    (nanos ^ std::process::id().rotate_left(16)) & 0xffffff
});

/// An empty or malformed optional field (`"setup": ""`) reads as absent instead of failing the reply.
fn lenient<'de, D: serde::Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(serde_json::Value::deserialize(d)?).ok())
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
    Empty,
}

/// Solves one assignment into a DOCX and returns its summary line.
pub async fn run_solve(inputs: &[PathBuf], opts: SolveOptions) -> Result<String> {
    let stem = inputs[0].file_stem().and_then(|s| s.to_str()).unwrap_or("solved").to_string();
    let tag = if opts.tag.is_empty() { String::new() } else { format!("[{}] ", opts.tag) };
    let read = read_inputs(inputs, opts.max_pages)?;
    if let Some(ev) = &opts.events { ev.send(Event::Reading { pages: read.images.len() / 2 }) }
    let work = tempfile::tempdir()?;
    // declared after `work`, so the container goes before its mounted dir does
    let _linux = crate::linux::Container::start(work.path()).await?;
    let mut configs = load_configurations()?;
    configs.retain(|_, c| c.installed());

    println!("{tag}🔍 Extracting questions with {} ({} image(s))…", crate::ai::describe(Tier::Smart), read.images.len() / 2);
    let photos = crate::ai::sees_images(Tier::Smart);
    let mut parts = vec![Part::Text(extract_prompt(&read.text, &lang_names(&configs, |c| !c.image), &lang_names(&configs, |c| c.session), photos))];
    if photos { parts.extend(read.images) }
    let permit = opts.sem.acquire().await.expect("semaphore closed");
    let mut extracted: Extracted = ask_json(Tier::Smart, &parts).await.context("extracting questions")?;
    drop(permit);
    // a document's setup also holds for later groups that bring none of their own
    let mut last_setup: Option<Setup> = None;
    for group in &mut extracted.groups {
        group.setup = group.setup.take().filter(|s| !s.code.trim().is_empty()).or_else(|| last_setup.clone());
        last_setup = group.setup.clone();
    }
    assign_labels(&mut extracted.groups);
    let doc = normalized(&read.text);
    // the text first: a bare title ("3") also matches numbered notes and headings before the question
    let question_pos: HashMap<String, usize> = extracted.groups.iter().flat_map(|g| &g.questions)
        .filter_map(|q| Some((q.title.clone(), text_pos(&doc, &q.text).or_else(|| title_pos(&doc, &q.title))?)))
        .collect();
    let total: usize = extracted.groups.iter().map(|g| g.questions.len()).sum();
    if let Some(ev) = &opts.events {
        let mut items: Vec<&Question> = extracted.groups.iter().flat_map(|g| &g.questions).collect();
        items.sort_by_key(|q| question_pos.get(&q.title).copied().unwrap_or(usize::MAX));
        let items = items.iter().map(|q| Item { id: q.title.clone(), title: q.text.trim().chars().take(3000).collect() }).collect();
        ev.send(Event::Found { items });
    }
    let group_count = extracted.groups.len();
    println!("{tag}📝 {} question(s) in {} group(s), solving with {}", total, group_count, crate::ai::describe(Tier::Mid));

    let mut langs = describe_langs(&configs);
    if !extracted.language.trim().is_empty() {
        langs.push_str(&format!("\nThe document's programs are written in {}: use it for every program unless a question names another language. Tasks a person does by hand rather than run still get lang null.", extracted.language.trim()));
    }
    let doc_lang = Some(extracted.language.trim().to_lowercase()).filter(|l| configs.get(l).is_some_and(|c| !c.session && !c.image));
    let langs = Arc::new(langs);
    let configs = Arc::new(configs);
    let data_files = Arc::new(read.data_files);
    let opts = Arc::new(opts);
    let mut set = JoinSet::new();
    for (n, group) in extracted.groups.into_iter().enumerate() {
        let dir = work.path().join(format!("g{}", n + 1));
        let (configs, langs, doc_lang, data_files, opts, tag) = (configs.clone(), langs.clone(), doc_lang.clone(), data_files.clone(), opts.clone(), tag.clone());
        set.spawn(crate::ai::inherit(async move {
            tokio::time::sleep(std::time::Duration::from_millis(n as u64)).await;
            let _permit = opts.sem.acquire().await.expect("semaphore closed");
            let titles = group.questions.iter().map(|q| q.title.clone()).collect::<Vec<_>>().join(", ");
            let progress = format!("{tag}[{}/{group_count}]", n + 1);
            tracing::info!("{progress} Solving: {titles}");
            let started = std::time::Instant::now();
            let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
            let result = solve_group(session_id, &group, &dir, &configs, &langs, doc_lang.as_deref(), &data_files, &opts).await;
            let secs = started.elapsed().as_secs_f32();
            match &result {
                Ok(items) => {
                    let failed = items.iter().filter(|(_, s, ..)| matches!(s, Status::Failed)).count();
                    let failed = if failed > 0 { format!(", {failed} failed") } else { String::new() };
                    tracing::info!("{progress} Done: {titles}, {} answer(s){failed} ({secs:.1}s)", items.len());
                }
                Err(e) => {
                    tracing::warn!("{progress} Failed: {titles} ({secs:.1}s): {e:#}");
                    if let Some(ev) = &opts.events { group.questions.iter().for_each(|q| ev.step(&q.title, "failed", format!("{e:#}"))) }
                }
            }
            let first = group.questions.first().map(|q| q.title.clone()).unwrap_or_default();
            (n, result.unwrap_or_else(|e| {
                let item = PackItem { heading: Some(titles.clone()), answer: format!("[solve failed: {e:#}]"), ..Default::default() };
                vec![(item, Status::Failed, titles, first)]
            }))
        }));
    }
    let mut results: Vec<_> = set.join_all().await;
    results.sort_by_key(|(n, _)| *n);
    let mut results: Vec<_> = results.into_iter().flat_map(|(_, r)| r).collect();
    // groups can interleave (Q8 on its own between Q7 and Q9), so restore document order per question
    if results.iter().all(|(.., title)| question_pos.contains_key(title)) {
        results.sort_by_key(|(.., title)| question_pos[title]);
    }

    let mut items = Vec::new();
    let (mut done, mut failed, mut no_shot) = (0, Vec::new(), Vec::new());
    for (item, status, label, _) in results {
        match status {
            Status::Done => done += 1,
            Status::NoScreenshot => no_shot.push(label),
            Status::Failed => failed.push(label),
            Status::Empty => { failed.push(label); continue }
        }
        items.push(item);
    }

    let out = opts.output.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_solved"))).with_extension("docx");
    let title = opts.doc_title.clone().unwrap_or(stem);
    crate::docx_gen::generate_docx(&out, &title, opts.doc_text.as_deref(), &items, opts.page_break, &opts.theme)?;
    if let Some(ev) = &opts.events {
        ev.done(out.file_name().map_or("solved.docx".into(), |n| n.to_string_lossy().into_owned()), out.clone());
    }
    let summary = format!("{} done, {} failed{}, {} without output{} → {}", done,
        failed.len(), list(&failed), no_shot.len(), list(&no_shot), out.display());
    println!("{tag}Done: {summary}");
    Ok(summary)
}

/// An interrupted run never reaches its groups' cleanups or container drops, so undo them here:
/// every session namespace of this run starts with the same prefix.
pub async fn cleanup_after_interrupt() {
    crate::linux::remove_all();
    let ns = format!("sc{:06x}", *RUN_ID);
    for cleanup in load_configurations().unwrap_or_default().values().filter_map(|c| c.cleanup.as_ref()) {
        let _ = crate::runner::run_piped(&crate::runner::with_ns(cleanup, &ns), "", &std::env::temp_dir()).await;
    }
}

fn list(labels: &[String]) -> String {
    if labels.is_empty() { String::new() } else { format!(" ({})", labels.join(", ")) }
}

#[allow(clippy::too_many_arguments)]
async fn solve_group(
    n: usize,
    group: &Group,
    dir: &Path,
    configs: &HashMap<String, LangConfig>,
    langs: &str,
    doc_lang: Option<&str>,
    data_files: &[PathBuf],
    opts: &SolveOptions,
) -> Result<Vec<(PackItem, Status, String, String)>> {
    std::fs::create_dir_all(dir)?;
    for file in data_files {
        std::fs::copy(file, dir.join(file.file_name().context("bad data file name")?))?;
    }
    let ev = opts.events.as_ref();
    if let Some(ev) = ev { group.questions.iter().for_each(|q| ev.step(&q.title, "writing", "")) }
    let mut solved = solve_questions(n, group, langs).await?;
    solved.iter_mut().for_each(|s| prepare(s, group, configs));
    if let Some(ev) = ev { solved.iter().for_each(|s| send_code(ev, s, "running", "")) }

    let ns = format!("sc{:06x}{n}_", *RUN_ID);
    let mut runs = run_all(n, &ns, group, &mut solved, configs, dir, None, Runs::default()).await?;
    let redone: Vec<(usize, Solved)> = review(n, group, &solved, &runs, configs, langs, doc_lang, dir, opts.check).await.into_iter()
        .filter_map(|mut s| {
            prepare(&mut s, group, configs);
            let qi = solved.iter().position(|x| x.title == s.title)?;
            let kept = keeps_kind(&solved[qi], &s, configs);
            if !kept { tracing::warn!("[session {n}] the redo of {} changed what kind of answer it is, keeping the original", s.title) }
            kept.then_some((qi, s))
        })
        .collect();
    if !redone.is_empty() {
        let broken_before: HashMap<usize, bool> = redone.iter().map(|(qi, _)| (*qi, broken(&solved, *qi, &runs, configs))).collect();
        let mut originals = HashMap::new();
        for (qi, s) in redone {
            originals.insert(qi, std::mem::replace(&mut solved[qi], s));
        }
        let changed: HashSet<usize> = originals.keys().copied().collect();
        if let Some(ev) = ev { changed.iter().for_each(|qi| send_code(ev, &solved[*qi], "fixing", "found a problem, fixing it")) }
        count(&STATS.redone, changed.len());
        runs = run_all(n, &ns, group, &mut solved, configs, dir, Some(&changed), runs).await?;
        // a redo that breaks what used to run is worse than the original
        let worse: HashSet<usize> = changed.into_iter().filter(|qi| !broken_before[qi] && broken(&solved, *qi, &runs, configs)).collect();
        if !worse.is_empty() {
            let titles: Vec<&str> = worse.iter().map(|qi| solved[*qi].title.as_str()).collect();
            tracing::warn!("[session {n}] review: the redo of {} broke it, keeping the original", titles.join(", "));
            for qi in &worse {
                solved[*qi] = originals.remove(qi).expect("replaced above");
            }
            runs = run_all(n, &ns, group, &mut solved, configs, dir, Some(&worse), runs).await?;
        }
    }
    let Runs { transcripts, outputs, images } = runs;
    count(&STATS.answers, solved.iter().map(|q| q.answers.len()).sum());

    let answers: Vec<(usize, &Solved, usize, &Answer)> = solved.iter().enumerate()
        .flat_map(|(qi, q)| q.answers.iter().enumerate().map(move |(i, a)| (qi, q, i, a)))
        .collect();
    let mut items = Vec::new();
    let group_code = answers.iter()
        .flat_map(|(.., a)| a.files.iter().map(|f| format!("{}:\n{}", f.name, f.code)).chain(Some(format!("{}:\n{}", a.heading, a.code))))
        .collect::<Vec<_>>().join("\n\n");

    for (idx, (qi, q, i, a)) in answers.iter().enumerate() {
        let question = group.questions.iter().find(|x| x.title == q.title);
        let mut term = opts.term.clone();
        term.no_title_bar = *i > 0;
        let mut item = PackItem {
            // a corrected answer continues under the heading of the one it corrects
            heading: Some(question.map_or_else(|| a.heading.replace(" (corrected)", ""), |q| q.label.clone())),
            parts: a.parts.clone(),
            content: a.code.clone(),
            answer: plain_text(&a.answer),
            question: question.filter(|_| (opts.questions || opts.theme.show_question) && *i == 0).map(question_text).unwrap_or_default(),
            ..Default::default()
        };
        let mut status = Status::Done;
        let output = match a.lang.as_deref() {
            None => None,
            Some(lang) => match configs.get(lang) {
                None => {
                    tracing::warn!("[{}] language {lang} isn't configured, no screenshot", a.heading);
                    status = Status::NoScreenshot;
                    None
                }
                Some(cfg) if cfg.image => {
                    match images.get(&(*qi, *i)) {
                        Some(Ok(png)) => {
                            let img = image::load_from_memory(png)?;
                            item.shots = vec![Shot { width: img.width(), height: img.height(), bytes: png.clone() }];
                            if cfg.hide_code { item.content.clear(); }
                        }
                        failed => {
                            let e = failed.and_then(|r| r.as_ref().err()).map_or("not drawn", String::as_str);
                            item.answer = format!("{}\n[failed to render: {e}]", item.answer).trim().to_string();
                            status = Status::NoScreenshot;
                        }
                    }
                    None
                }
                Some(cfg) if cfg.session => {
                    let outs = transcripts.get(&idx).cloned().unwrap_or_default();
                    // keys typed into a full-screen program (vi) can't be shown on a screenshot, so the answer stays text
                    let typed_into = outs.iter().zip(steps_of(&a.code)).enumerate()
                        .any(|(k, (out, step))| full_screen(out) && (k + 1 < outs.len() || step.lines().count() > 1));
                    if typed_into { None } else {
                        let steps = merge_short_steps(outs.into_iter().map(|o| if full_screen(&o) { final_screen(&o) } else { o }).collect());
                        for (s, step) in steps.iter().enumerate() {
                            let mut step_term = term.clone();
                            step_term.no_title_bar |= s > 0;
                            item.shots.extend(crate::pack::screenshots(None, &tidy_paths(step, dir), &step_term, opts.trim)?);
                        }
                        // typed at the prompt, so already in the screenshot (even when wrapped)
                        item.content.clear();
                        Some(tidy_paths(&steps.join("\n"), dir))
                    }
                }
                Some(_) => match outputs.get(&(*qi, *i)) {
                    // a session in the group builds and runs this file itself
                    None => None,
                    Some((_, out)) if full_screen(out) && !a.stdin.is_empty() => None,
                    Some((cmd, out)) => {
                        let out = tidy_paths(&if full_screen(out) { final_screen(out) } else { out.clone() }, dir);
                        item.shots = crate::pack::screenshots(Some(cmd), &out, &term, opts.trim)?;
                        Some(out)
                    }
                },
            },
        };
        // code already visible in the screenshot (typed at a prompt, cat'ed) isn't repeated above it
        if output.as_deref().is_some_and(|out| shows_code(out, &a.code)) {
            item.content.clear();
        }
        // models sometimes repeat the answer's own code as a file; sample data (test.txt) is used but not shown
        let is_code = |f: &&NamedFile| Path::new(&f.name).extension().and_then(|e| e.to_str()).is_some_and(|e| configs.contains_key(e));
        let files: Vec<&NamedFile> = a.files.iter().filter(|f| f.code.trim() != a.code.trim()).filter(is_code).collect();
        if !files.is_empty() {
            let named = files.len() > 1;
            let files = files.iter().map(|f| if named { format!("{}:\n{}", f.name, f.code) } else { f.code.clone() });
            item.content = files.chain(Some(item.content.clone()).filter(|c| !c.is_empty())).collect::<Vec<_>>().join("\n\n");
        }
        if let (true, Some(out)) = (a.answer_after_run, &output) {
            let q_text = question.map(question_text).unwrap_or_default();
            match ask(Tier::Mid, &[Part::Text(after_run_prompt(&q_text, &group_code, out))], false).await {
                Ok(text) => {
                    item.answer_after = plain_text(&text);
                    // both end up below the screenshot, and the after-run text already answers it
                    item.answer.clear();
                }
                Err(e) => tracing::warn!("[{}] written answer failed: {e:#}", a.heading),
            }
        }
        if item.content.is_empty() && item.answer.is_empty() && item.answer_after.is_empty() && item.shots.is_empty() {
            tracing::warn!("[{}] nothing to show (lang {:?}, {} code lines, {} files, {} session steps), left out",
                a.heading, a.lang, a.code.lines().count(), a.files.len(), transcripts.get(&idx).map_or(0, Vec::len));
            status = Status::Empty;
        }
        if let Some(ev) = ev {
            if let Some(out) = &output { ev.send(Event::Output { id: q.title.clone(), text: out.clone() }) }
            item.shots.iter().for_each(|s| ev.shot(&q.title, s.bytes.clone()));
        }
        items.push((item, status, a.heading.clone(), q.title.clone()));
    }
    let answered = answered_titles(&solved);
    for q in group.questions.iter().filter(|q| !answered.contains(&normalized(&q.title))) {
        let item = PackItem { heading: Some(q.label.clone()), answer: "[solve failed: the model gave no answer]".into(), ..Default::default() };
        items.push((item, Status::Failed, q.label.clone(), q.title.clone()));
    }
    if let Some(ev) = ev {
        for q in &group.questions {
            let ok = items.iter().any(|(_, st, _, t)| *t == q.title && matches!(st, Status::Done | Status::NoScreenshot));
            if ok { ev.step(&q.title, "ok", "") } else { ev.step(&q.title, "failed", "no answer came out") }
        }
    }
    Ok(items)
}

/// The web UI's view of an answer: all its code (or written text) and what happens to it next.
fn send_code(ev: &Emitter, s: &Solved, state: &'static str, note: &str) {
    let written = s.answers.iter().all(|a| a.lang.is_none());
    let code = s.answers.iter().map(|a| if a.lang.is_none() { a.answer.as_str() } else { a.code.as_str() }).collect::<Vec<_>>().join("\n\n");
    ev.send(Event::Code { id: s.title.clone(), written, code });
    ev.step(&s.title, state, note);
}

/// Asks for the group's answers a few questions at a time, since big groups in one call outlast gateway timeouts.
async fn solve_questions(n: usize, group: &Group, langs: &str) -> Result<Vec<Solved>> {
    let mut solved: Vec<Solved> = Vec::new();
    for chunk in group.questions.chunks(QUESTIONS_PER_CALL) {
        let mut todo: Vec<&Question> = chunk.iter().collect();
        // models sometimes answer only some of the questions; ask again for the rest
        for attempt in 0..=crate::ai::max_retries() {
            let part = serde_json::to_string_pretty(&serde_json::json!({"setup": group.setup, "questions": todo}))?;
            let earlier = if solved.is_empty() { String::new() } else { serde_json::to_string(&solved)? };
            let mut chunk_solved: Vec<Solved> = ask_json(Tier::Mid, &[Part::Text(solve_prompt(&part, &earlier, langs))]).await
                .context("solving")?;
            keep_asked(n, &mut chunk_solved, &todo);
            let complete = chunk_solved.len() >= todo.len();
            solved.extend(chunk_solved);
            let answered = answered_titles(&solved);
            todo.retain(|q| !answered.contains(&normalized(&q.title)));
            if complete || todo.is_empty() || attempt == crate::ai::max_retries() { break }
            count(&STATS.asked_again, 1);
            tracing::warn!("[session {n}] no answer for {}, asking again {}/{}", todo.iter().map(|q| q.title.as_str()).collect::<Vec<_>>().join(", "), attempt + 1, crate::ai::max_retries());
        }
    }
    Ok(solved)
}

/// Keeps replies to the asked questions, under their exact titles, without empty answers.
fn keep_asked(n: usize, replies: &mut Vec<Solved>, asked: &[&Question]) {
    // models sometimes return the format example or a question nobody asked
    // models answer to the label they were shown (15) as often as to the title (Practice Q15)
    replies.retain_mut(|s| match asked.iter().find(|q| [&q.title, &q.label].iter().any(|t| normalized(t) == normalized(&s.title))) {
        Some(q) => {
            s.title = q.title.clone();
            // an answer with nothing in it (no language, code or text) counts as no answer
            s.answers.retain(|a| a.lang.is_some() || !a.code.trim().is_empty() || !a.answer.trim().is_empty());
            !s.answers.is_empty()
        }
        None => {
            tracing::warn!("[session {n}] dropped an answer to {:?}, which wasn't asked", s.title);
            false
        }
    });
    // models sometimes split one question into an entry per part
    let mut merged: Vec<Solved> = Vec::new();
    for s in replies.drain(..) {
        match merged.iter_mut().find(|m| m.title == s.title) {
            Some(m) => m.answers.extend(s.answers),
            None => merged.push(s),
        }
    }
    *replies = merged;
}

/// Headings, code the model escaped or mangled, and languages it got wrong.
fn prepare(s: &mut Solved, group: &Group, configs: &HashMap<String, LangConfig>) {
    let question = group.questions.iter().find(|q| normalized(&q.title) == normalized(&s.title));
    // " (corrected)" only follows the answer it corrects
    if s.answers.iter().all(|a| a.label.ends_with(" (corrected)")) {
        s.answers.iter_mut().for_each(|a| a.label = a.label.trim_end_matches(" (corrected)").to_string());
    }
    // part headings only when the question has parts that get separate answers, never a lone 12(b)
    let own = s.answers.iter().filter(|a| !a.label.ends_with(" (corrected)")).count();
    let split = question.is_some_and(|q| !q.parts.is_empty()) && own > 1;
    for a in &mut s.answers {
        (a.heading, a.slug) = match question {
            Some(q) => (heading(&q.label, &a.label, split), heading(&q.label, &a.label, own > 1)),
            None => (a.label.clone(), a.label.clone()),
        };
        a.parts = if split && question.is_some() { part_numbers(&a.label) } else { Vec::new() };
        unescape_code(&mut a.code);
        a.files.iter_mut().for_each(|f| unescape_code(&mut f.code));
        // code quoted from the question is given code, shown as it runs: its errors are the point
        let code = normalized(&a.code);
        if code.len() >= 20 && !a.label.ends_with(" (corrected)") && question.is_some_and(|q| normalized(&question_text(q)).contains(&code)) {
            a.expects_error = true;
        }
        // some models write the JSON null as the string "null"
        a.lang = a.lang.take().filter(|l| !matches!(l.as_str(), "" | "null" | "none"));
        if a.lang.is_none() {
            a.lang = guess_lang(&a.code).filter(|l| configs.contains_key(*l)).map(String::from);
        }
        let cfg = a.lang.as_deref().and_then(|l| configs.get(l));
        // nothing to run, or keys typed into an editor: a written answer, never a screenshot
        if a.code.trim().is_empty() && a.files.is_empty() || cfg.is_some_and(|c| !c.session && !c.image) && opens_editor(&a.code) {
            a.lang = None;
        }
        if let Some(c) = cfg.filter(|c| c.session) {
            // `\G` already ends the statement, so a `;` after it runs an empty one ("No query specified")
            a.code = a.code.replace("\\G;", "\\G");
            a.code = without_comments(&a.code, c);
        }
        if a.lang.as_deref() == Some("mysql") { a.code = with_delimiters(&a.code) }
    }
}

/// Writes every answer's files and each file answer, before sessions run, so a session can compile or
/// run another answer's file. Returns the names taken and each file answer's name by answer index.
fn write_files(solved: &[Solved], dir: &Path, configs: &HashMap<String, LangConfig>) -> Result<(HashSet<String>, Vec<Option<String>>)> {
    let mut names = HashSet::new();
    let mut file_names = Vec::new();
    for a in solved.iter().flat_map(|q| &q.answers) {
        for f in &a.files {
            let rel = Path::new(&f.name);
            if !rel.components().all(|c| matches!(c, std::path::Component::Normal(_))) { bail!("bad file name {:?}", f.name) }
            std::fs::create_dir_all(dir.join(rel).parent().unwrap_or(dir))?;
            std::fs::write(dir.join(rel), &f.code)?;
        }
        let name = a.lang.as_deref()
            .filter(|l| configs.get(*l).is_some_and(|c| !c.session && !c.image))
            .map(|l| java_class(l, &a.code).filter(|n| names.insert(n.clone())).unwrap_or_else(|| unique_name(&a.slug, l, &mut names)));
        if let Some(name) = &name {
            std::fs::write(dir.join(name), &a.code)?;
        }
        file_names.push(name);
    }
    Ok((names, file_names))
}

/// What running a group left: every answer's output and drawing.
#[derive(Default)]
struct Runs {
    /// Step transcripts by answer index.
    transcripts: HashMap<usize, Vec<String>>,
    /// (command, output) by (question, answer) index.
    outputs: HashMap<(usize, usize), (String, String)>,
    /// PNG or why it failed, by (question, answer) index.
    images: HashMap<(usize, usize), Result<Vec<u8>, String>>,
}

/// Writes the group's files and runs every session (they share state), then the file answers and
/// diagrams of the questions in `only`, or all of them; `previous` keeps the rest.
#[allow(clippy::too_many_arguments)]
async fn run_all(
    n: usize,
    ns: &str,
    group: &Group,
    solved: &mut [Solved],
    configs: &HashMap<String, LangConfig>,
    dir: &Path,
    only: Option<&HashSet<usize>>,
    previous: Runs,
) -> Result<Runs> {
    let (mut names, file_names) = write_files(solved, dir, configs)?;
    let written = snapshot(dir);
    let transcripts = run_sessions_fixed(n, ns, group, solved, configs, dir, &written).await;
    let Runs { mut outputs, mut images, .. } = previous;
    if let Some(only) = only {
        outputs.retain(|(qi, _), _| !only.contains(qi));
        images.retain(|(qi, _), _| !only.contains(qi));
    }
    outputs.extend(run_files(solved, group, configs, dir, &file_names, only).await?);
    for (qi, q) in solved.iter().enumerate().filter(|(qi, _)| only.is_none_or(|o| o.contains(qi))) {
        for (ai, a) in q.answers.iter().enumerate() {
            let Some((lang, cfg)) = a.lang.as_deref().and_then(|l| Some((l, configs.get(l)?))).filter(|(_, c)| c.image) else { continue };
            let png = render_diagram(cfg, dir, &unique_name(&a.slug, lang, &mut names), &a.code).await;
            images.insert((qi, ai), png.map_err(|e| format!("{e:#}")));
        }
    }
    Ok(Runs { transcripts, outputs, images })
}

/// An answer to question `qi` crashed, failed or printed nothing without meaning to.
fn broken(solved: &[Solved], qi: usize, runs: &Runs, configs: &HashMap<String, LangConfig>) -> bool {
    let start: usize = solved[..qi].iter().map(|q| q.answers.len()).sum();
    solved[qi].answers.iter().enumerate().filter(|(_, a)| !a.expects_error).any(|(ai, a)| {
        let cfg = a.lang.as_deref().and_then(|l| configs.get(l));
        if cfg.is_some_and(|c| c.session) {
            runs.transcripts.get(&(start + ai)).is_some_and(|t| session_failed(&t.join("\n")))
        } else if cfg.is_some_and(|c| c.image) {
            runs.images.get(&(qi, ai)).is_some_and(Result::is_err)
        } else {
            runs.outputs.get(&(qi, ai)).is_some_and(|(_, out)| out.trim().is_empty() || crashed(out))
        }
    })
}

/// Runs the group's sessions; a session error the question isn't about is the model's mistake,
/// so those answers are fixed and the group rerun. Returns each answer's step transcripts by answer index.
async fn run_sessions_fixed(n: usize, ns: &str, group: &Group, solved: &mut [Solved], configs: &HashMap<String, LangConfig>, dir: &Path, written: &HashSet<PathBuf>) -> HashMap<usize, Vec<String>> {
    prune(dir, written);
    let mut transcripts = run_group_sessions(n, ns, group, solved, configs, dir).await;
    for round in 1..=FIXES {
        let broken: Vec<(usize, usize, String)> = solved.iter().enumerate()
            .flat_map(|(qi, q)| q.answers.iter().enumerate().map(move |(ai, a)| (qi, ai, a, q)))
            .enumerate()
            .filter(|(_, (_, _, a, q))| !a.expects_error && !q.answers.iter().any(|x| x.label.contains("(corrected)")))
            .filter_map(|(idx, (qi, ai, _, _))| Some((qi, ai, transcripts.get(&idx)?.join("\n"))))
            .filter(|(_, _, out)| session_failed(out))
            .collect();
        if broken.is_empty() { break }
        for (qi, ai, out) in broken {
            let q_text = group.questions.iter().find(|x| x.title == solved[qi].title).map(question_text).unwrap_or_default();
            let lang = &solved[qi].answers[ai].lang;
            let before = solved.iter().enumerate()
                .flat_map(|(i, q)| q.answers.iter().enumerate().map(move |(j, x)| ((i, j), x)))
                .take_while(|(pos, _)| *pos < (qi, ai))
                .filter(|(_, x)| &x.lang == lang)
                .map(|(_, x)| x.code.as_str()).collect::<Vec<_>>().join("\n\n");
            let a = &mut solved[qi].answers[ai];
            count(&STATS.fixes, 1);
            tracing::warn!("[{}] session step failed, asking for a fix ({round}/{FIXES})", a.heading);
            match ask(Tier::Mid, &[Part::Text(fix_prompt(&q_text, &before, &a.code, &out))], false).await {
                Ok(fixed) => {
                    a.code = strip_fences(&fixed).to_string();
                    if let Some(c) = a.lang.as_deref().and_then(|l| configs.get(l)) { a.code = without_comments(&a.code, c) }
                    if a.lang.as_deref() == Some("mysql") { a.code = with_delimiters(&a.code) }
                }
                Err(e) => tracing::warn!("[{}] fix failed: {e:#}", a.heading),
            }
        }
        prune(dir, written);
        transcripts = run_group_sessions(n, ns, group, solved, configs, dir).await;
    }
    transcripts
}

/// Models comment typed code anyway, and each comment line would show as its own step.
fn without_comments(code: &str, cfg: &LangConfig) -> String {
    code.lines().filter(|l| !cfg.comments.iter().any(|m| l.trim_start().starts_with(m.as_str()))).collect::<Vec<_>>().join("\n")
}

/// Every path under `dir`, without following links.
fn snapshot(dir: &Path) -> HashSet<PathBuf> {
    let mut paths = HashSet::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) { todo.push(entry.path()) }
            paths.insert(entry.path());
        }
    }
    paths
}

/// Removes what earlier runs left in `dir`, so a rerun starts from the files in `keep` only.
fn prune(dir: &Path, keep: &HashSet<PathBuf>) {
    for path in snapshot(dir).into_iter().filter(|p| !keep.contains(p)) {
        let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
        let _ = if is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
    }
}

/// Runs each file answer that no session builds itself (of the questions in `only`, when given) and
/// repairs crashes that aren't the point of the question; only a fix that runs cleanly replaces the
/// code, so a bad fix can't make it worse. Returns (command, output) by (question, answer) index.
async fn run_files(
    solved: &mut [Solved],
    group: &Group,
    configs: &HashMap<String, LangConfig>,
    dir: &Path,
    file_names: &[Option<String>],
    only: Option<&HashSet<usize>>,
) -> Result<HashMap<(usize, usize), (String, String)>> {
    let positions: Vec<(usize, usize)> = solved.iter().enumerate()
        .flat_map(|(qi, q)| (0..q.answers.len()).map(move |ai| (qi, ai)))
        .collect();
    let mut outputs = HashMap::new();
    for (&(qi, ai), filename) in positions.iter().zip(file_names) {
        let Some(filename) = filename else { continue };
        if only.is_some_and(|o| !o.contains(&qi)) || session_uses(solved, configs, filename) { continue }
        let q_text = group.questions.iter().find(|x| x.title == solved[qi].title).map(question_text).unwrap_or_default();
        // a failing given program is followed by its "(corrected)" answer, so only our own crashes are repaired
        let has_correction = solved[qi].answers.iter().any(|x| x.label.contains("(corrected)"));
        let a = &mut solved[qi].answers[ai];
        let path = dir.join(filename);
        let stdin = Some(a.stdin.clone()).filter(|s| !s.is_empty());
        // the solving model already gave the stdin; a second model guessing it types prose into the program
        let (mut cmd, mut out, mut exit_err) = crate::pack::run_with_input(&path, dir, filename, &a.code, stdin.as_deref(), &a.args, AiInput::Never, None).await;
        if !has_correction && !a.expects_error {
            let (mut code, mut error) = (a.code.clone(), format!("$ {cmd}\n{out}"));
            for attempt in 1..=FIXES {
                if !exit_err && !crashed(&out) { break }
                count(&STATS.fixes, 1);
                tracing::warn!("[{}] program failed, asking for a fix ({attempt}/{FIXES})", a.heading);
                match ask(Tier::Mid, &[Part::Text(fix_prompt(&q_text, "", &code, &error))], false).await {
                    Ok(fixed) => {
                        code = strip_fences(&fixed).to_string();
                        std::fs::write(&path, &code)?;
                        let run = crate::pack::run_with_input(&path, dir, filename, &code, stdin.as_deref(), &a.args, AiInput::Never, None).await;
                        error = format!("$ {}\n{}", run.0, run.1);
                        if !run.2 && !crashed(&run.1) {
                            (cmd, out, exit_err) = run;
                            a.code = code.clone();
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[{}] fix failed: {e:#}", a.heading);
                        break;
                    }
                }
            }
            std::fs::write(&path, &a.code)?;
        }
        if out.trim().is_empty() { tracing::warn!("[{}] the run printed nothing", a.heading) }
        outputs.insert((qi, ai), (cmd, out));
    }
    Ok(outputs)
}

/// Finds questions whose answers ran wrong (printed nothing or only 0, a missing file or argument, still
/// fail after fixes, fewer answers than parts, answered both whole and per part, not in the document's
/// language, or graded bad by `--check`) and has the model redo just those, told what went wrong.
/// Returns the redone questions.
#[allow(clippy::too_many_arguments)]
async fn review(
    n: usize,
    group: &Group,
    solved: &[Solved],
    runs: &Runs,
    configs: &HashMap<String, LangConfig>,
    langs: &str,
    doc_lang: Option<&str>,
    dir: &Path,
    check: Option<f32>,
) -> Vec<Solved> {
    const SHOWN: usize = 3000;
    let shown = |out: &str| -> String {
        let out = tidy_paths(&if full_screen(out) { final_screen(out) } else { out.to_string() }, dir).replace('\r', "");
        out.chars().take(SHOWN).collect()
    };
    let mut idx = 0;
    let mut entries: Vec<Entry> = Vec::new();
    for (qi, q) in solved.iter().enumerate() {
        let mut problems = Vec::new();
        let parted = |a: &&Answer| a.label.trim_end_matches(" (corrected)").contains('.');
        let own: Vec<&Answer> = q.answers.iter().filter(|a| !a.label.ends_with(" (corrected)")).collect();
        if own.iter().any(parted) && !own.iter().all(parted) {
            problems.push("it is answered both as a whole and per part: give one answer per part".to_string());
        }
        // written and session answers get one answer per part, so fewer means parts were skipped
        let parts = group.questions.iter().find(|x| x.title == q.title).map_or(0, |x| x.parts.len());
        let per_part = q.answers.iter().all(|a| a.lang.as_deref().and_then(|l| configs.get(l)).is_none_or(|c| c.session));
        if per_part && parts > 1 && own.len() < parts {
            problems.push(format!("it has {parts} parts but only {} answer(s): answer every part, one answer per part", own.len()));
        }
        let mut answers = Vec::new();
        let mut pictures = Vec::new();
        for (ai, a) in q.answers.iter().enumerate() {
            let cfg = a.lang.as_deref().and_then(|l| configs.get(l));
            let printed = match cfg {
                None => "(a written answer, not run)".to_string(),
                Some(c) if c.image => match runs.images.get(&(qi, ai)) {
                    Some(Ok(png)) => {
                        pictures.push(png.clone());
                        "(drawn: the next attached picture)".to_string()
                    }
                    _ => "(failed to draw)".to_string(),
                },
                Some(c) if c.session => {
                    let out = runs.transcripts.get(&idx).map(|t| t.join("\n")).unwrap_or_default();
                    if !a.expects_error && session_failed(&out) { problems.push(format!("{} still fails after fixes", a.heading)) }
                    shown(&out)
                }
                Some(_) => match runs.outputs.get(&(qi, ai)) {
                    None => "(built and run by a session answer)".to_string(),
                    Some((cmd, out)) => {
                        if !a.expects_error {
                            if let Some(p) = sketchy(out) { problems.push(format!("{}: {p}", a.heading)) }
                        }
                        shown(&format!("$ {cmd}\n{out}"))
                    }
                },
            };
            if let (Some(doc), Some(lang)) = (doc_lang, a.lang.as_deref()) {
                if lang != doc && cfg.is_some_and(|c| !c.session && !c.image) {
                    problems.push(format!("{} is written in {lang}, but the document's programs are {doc}: use {doc} unless the question asks for {lang}", a.heading));
                }
            }
            answers.push(serde_json::json!({"label": a.label, "lang": a.lang, "code": a.code, "files": a.files.iter().map(|f| &f.name).collect::<Vec<_>>(),
                "stdin": a.stdin, "args": a.args, "expects_error": a.expects_error, "answer": a.answer, "printed": printed}));
            idx += 1;
        }
        entries.push((problems, answers, pictures));
    }
    if let Some(min) = check {
        grade(n, group, solved, &mut entries, min).await;
    }
    let flagged: Vec<(usize, serde_json::Value)> = entries.into_iter().enumerate()
        .filter(|(_, (problems, ..))| !problems.is_empty())
        .map(|(qi, (problems, answers, _))| (qi, serde_json::json!({"title": solved[qi].title, "problems": problems, "answers": answers})))
        .collect();

    let mut set = JoinSet::new();
    for chunk in flagged.chunks(QUESTIONS_PER_CALL) {
        let titles: Vec<String> = chunk.iter().map(|(qi, _)| solved[*qi].title.clone()).collect();
        let questions: Vec<&Question> = group.questions.iter().filter(|q| titles.contains(&q.title)).collect();
        let checked: Vec<&serde_json::Value> = chunk.iter().map(|(_, c)| c).collect();
        let first = chunk[0].0;
        let (Ok(questions), Ok(checked), Ok(earlier)) = (
            serde_json::to_string_pretty(&serde_json::json!({"setup": group.setup, "questions": questions})),
            serde_json::to_string_pretty(&checked),
            if first == 0 { Ok(String::new()) } else { serde_json::to_string(&solved[..first]) },
        ) else { continue };
        tracing::warn!("[session {n}] redoing {}: {}", titles.join(", "), chunk.iter().map(|(_, c)| c["problems"].to_string()).collect::<Vec<_>>().join("; "));
        let prompt = review_prompt(&questions, &checked, &earlier, langs);
        // a redo is the second chance after an answer already went wrong, so it gets the stronger model
        set.spawn(crate::ai::inherit(async move { (titles, ask_json::<Vec<Solved>>(Tier::Smart, &[Part::Text(prompt)]).await) }));
    }
    let mut redone: Vec<Solved> = Vec::new();
    for (titles, reply) in set.join_all().await {
        match reply {
            Ok(mut replies) => {
                let asked: Vec<&Question> = group.questions.iter().filter(|q| titles.contains(&q.title)).collect();
                keep_asked(n, &mut replies, &asked);
                for s in replies {
                    if !redone.iter().any(|r| r.title == s.title) { redone.push(s) }
                }
            }
            Err(e) => tracing::warn!("[session {n}] redo failed: {e:#}"),
        }
    }
    redone
}

/// A question as `review` sees it: its problems, its answers with what they printed, and the pictures it drew.
type Entry = (Vec<String>, Vec<serde_json::Value>, Vec<Vec<u8>>);

/// `--check`: the SMART model grades every answer that ran, drew or was written, a few questions per call, and a
/// bad grade at least `min` sure becomes a problem of its question.
async fn grade(n: usize, group: &Group, solved: &[Solved], entries: &mut [Entry], min: f32) {
    #[derive(Deserialize)]
    struct Verdict {
        title: String,
        label: String,
        verdict: String,
        #[serde(default)]
        confidence: f32,
        #[serde(default)]
        problem: String,
    }
    let shown = |printed: &serde_json::Value| printed.as_str().is_some_and(|p| !p.starts_with("(built"));
    let gradable: Vec<usize> = (0..solved.len()).filter(|&qi| entries[qi].1.iter().any(|a| shown(&a["printed"]))).collect();
    let mut set = JoinSet::new();
    for chunk in gradable.chunks(QUESTIONS_PER_CALL) {
        let titles: Vec<&String> = chunk.iter().map(|&qi| &solved[qi].title).collect();
        let questions: Vec<&Question> = group.questions.iter().filter(|q| titles.contains(&&q.title)).collect();
        let answers: Vec<serde_json::Value> = chunk.iter()
            .map(|&qi| serde_json::json!({"title": solved[qi].title, "answers": entries[qi].1.iter().filter(|a| shown(&a["printed"])).collect::<Vec<_>>()}))
            .collect();
        let (Ok(questions), Ok(answers)) = (
            serde_json::to_string_pretty(&serde_json::json!({"setup": group.setup, "questions": questions})),
            serde_json::to_string_pretty(&answers),
        ) else { continue };
        let mut parts = vec![Part::Text(check_prompt(&questions, &answers))];
        parts.extend(chunk.iter().flat_map(|&qi| entries[qi].2.iter().map(|png| Part::Image("image/png", png.clone()))));
        set.spawn(crate::ai::inherit(async move { ask_json::<Vec<Verdict>>(Tier::Smart, &parts).await }));
    }
    for reply in set.join_all().await {
        let verdicts = match reply {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("[session {n}] check failed: {e:#}");
                continue;
            }
        };
        for v in verdicts {
            let Some(qi) = solved.iter().position(|s| normalized(&s.title) == normalized(&v.title)) else { continue };
            if v.verdict.trim().eq_ignore_ascii_case("good") {
                count(&STATS.good, 1);
                tracing::info!("[{}] check: good ({:.2})", v.label, v.confidence);
                continue;
            }
            count(&STATS.bad, 1);
            tracing::warn!("[{}] check: bad ({:.2}): {}", v.label, v.confidence, v.problem);
            if v.confidence >= min {
                entries[qi].0.push(format!("{}: {}", v.label, v.problem));
            }
        }
    }
}

/// What a run's output shows went wrong, when the question isn't about an error.
fn sketchy(out: &str) -> Option<&'static str> {
    let text = out.trim();
    let lower = text.to_lowercase();
    if text.is_empty() {
        Some("its run printed nothing")
    } else if text == "0" {
        Some("its run printed only 0, so there was nothing to work on")
    } else if crashed(text) {
        Some("it still crashes after fixes")
    } else if text.contains("[Timeout after") {
        Some("it timed out, waiting for input it was never given or running on: give it its stdin and let it finish")
    } else if ["no such ", "does not exist", "usage:"].iter().any(|m| lower.contains(m)) {
        Some("its run says a file, input or argument is missing: give it real ones, creating sample files first")
    } else {
        None
    }
}

/// A redo keeps the question's kind of answer: no code where there was only text, no lost diagram.
fn keeps_kind(old: &Solved, new: &Solved, configs: &HashMap<String, LangConfig>) -> bool {
    let has = |s: &Solved, image: bool| s.answers.iter().any(|a| a.lang.as_deref().and_then(|l| configs.get(l)).is_some_and(|c| c.image == image));
    (has(old, false) || !has(new, false)) && (!has(old, true) || has(new, true))
}

/// Reset, set up and run every session of the group, then clean up; nothing in between returns
/// early, so session state never outlives the group. Returns each answer's step transcripts by answer index.
async fn run_group_sessions(n: usize, ns: &str, group: &Group, solved: &[Solved], configs: &HashMap<String, LangConfig>, dir: &Path) -> HashMap<usize, Vec<String>> {
    let answers: Vec<(&Solved, usize, &Answer)> = solved.iter()
        .flat_map(|q| q.answers.iter().enumerate().map(move |(i, a)| (q, i, a)))
        .collect();
    let session_langs: HashSet<&str> = group.setup.iter().map(|s| s.lang.as_str())
        .chain(solved.iter().flat_map(|q| q.answers.iter().filter_map(|a| a.lang.as_deref())))
        .filter(|l| configs.get(*l).is_some_and(|c| c.session))
        .collect();
    for lang in &session_langs {
        if let Some(reset) = &configs[*lang].reset {
            if let Err(e) = crate::runner::run_piped(&crate::runner::with_ns(reset, ns), "", dir).await {
                tracing::warn!("[session {n}] {lang} reset failed: {e:#}");
            }
        }
    }
    if let Some(setup) = &group.setup {
        match configs.get(&setup.lang).filter(|c| c.session) {
            Some(cfg) => {
                let code = if setup.lang == "mysql" { with_delimiters(&setup.code) } else { setup.code.clone() };
                let code = crate::runner::namespaced(&code, &cfg.namespace, ns);
                if let Err(e) = crate::runner::run_piped(&crate::runner::with_ns(&cfg.run, ns), &code, dir).await {
                    tracing::warn!("[session {n}] setup failed: {e:#}");
                }
            }
            None => tracing::warn!("[session {n}] setup language {} is not a session language, skipped", setup.lang),
        }
    }
    let transcripts = run_sessions(ns, &answers, configs, dir).await;
    for lang in &session_langs {
        if let Some(cleanup) = &configs[*lang].cleanup {
            if let Err(e) = crate::runner::run_piped(&crate::runner::with_ns(cleanup, ns), "", dir).await {
                tracing::warn!("[session {n}] {lang} cleanup failed: {e:#}");
            }
        }
    }
    transcripts
}

/// A failed command or statement, leaving out MySQL's SIGNAL SQLSTATE '45000', which is always raised on purpose.
fn session_failed(out: &str) -> bool {
    crashed(out) || out.lines().any(|l| l.starts_with("ERROR ") && l.contains("): ") && !l.contains("(45000)"))
}

fn answered_titles(solved: &[Solved]) -> HashSet<String> {
    solved.iter().map(|s| normalized(&s.title)).collect()
}

/// A session in the group compiles or runs this file answer itself, so it is only shown.
fn session_uses(solved: &[Solved], configs: &HashMap<String, LangConfig>, file: &str) -> bool {
    solved.iter().flat_map(|q| &q.answers).any(|a| a.lang.as_deref().and_then(|l| configs.get(l)).is_some_and(|c| c.session) && a.code.contains(file))
}

/// The language of code the model gave none: a Graphviz diagram or a script with a shebang.
fn guess_lang(code: &str) -> Option<&'static str> {
    let code = code.trim();
    if ["digraph", "graph", "strict "].iter().any(|k| code.starts_with(k)) && code.ends_with('}') { return Some("dot") }
    let shebang = code.lines().next()?.strip_prefix("#!")?.trim_end();
    [("python", "py"), ("node", "js"), ("sh", "sh")].into_iter().find(|(k, _)| shebang.contains(k)).map(|(_, lang)| lang)
}

/// A script that opens an editor holds keys a person types, not something to run.
fn opens_editor(code: &str) -> bool {
    code.lines().any(|l| matches!(l.split_whitespace().next(), Some("vi" | "vim" | "nvim" | "nano" | "emacs")))
}

/// Runs each stretch of consecutive answers in the same session language as one session,
/// so state (tables, variables, cwd) carries over. Blank lines split an answer into steps.
/// Returns each answer's step transcripts by answer index.
async fn run_sessions(ns: &str, answers: &[(&Solved, usize, &Answer)], configs: &HashMap<String, LangConfig>, dir: &Path) -> HashMap<usize, Vec<String>> {
    let session_of = |a: &Answer| a.lang.as_deref().filter(|l| configs.get(*l).is_some_and(|c| c.session)).map(str::to_string);
    let mut transcripts = HashMap::new();
    let mut idx = 0;
    while idx < answers.len() {
        let Some(lang) = session_of(answers[idx].2) else { idx += 1; continue };
        let batch: Vec<usize> = (idx..answers.len()).take_while(|&j| session_of(answers[j].2).as_deref() == Some(lang.as_str())).collect();
        idx += batch.len();
        let cfg = &configs[&lang];
        let (argv, prompts, dir) = (crate::runner::with_ns(&cfg.run, ns), cfg.prompts.clone(), dir.to_path_buf());
        let steps: Vec<(usize, String)> = batch.iter()
            .flat_map(|&j| steps_of(&answers[j].2.code).into_iter().map(move |step| (j, step)))
            .collect();
        let chunks: Vec<String> = steps.iter().map(|(_, step)| crate::runner::namespaced(step, &cfg.namespace, ns)).collect();
        let result = tokio::task::spawn_blocking(move || crate::runner::run_session(&argv, &prompts, &chunks, &dir)).await;
        let outs = match result {
            Ok(Ok(outs)) => outs,
            Ok(Err(e)) => vec![format!("[Error starting {lang}: {e:#}]"); steps.len()],
            Err(e) => vec![format!("[Error: {e}]"); steps.len()],
        };
        for ((j, _), out) in steps.into_iter().zip(outs) {
            transcripts.entry(j).or_insert_with(Vec::new).push(crate::runner::unnamespaced(&out, ns));
        }
    }
    transcripts
}

/// Render an image answer; on failure ask the model to fix it, up to `FIXES` times.
async fn render_diagram(cfg: &LangConfig, dir: &Path, filename: &str, code: &str) -> Result<Vec<u8>> {
    let path = dir.join(filename);
    let mut code = code.to_string();
    for attempt in 1.. {
        std::fs::write(&path, &code)?;
        let err = match crate::runner::render_image(cfg, &path, dir).await {
            Ok(png) => return Ok(png),
            Err(e) if attempt > FIXES => return Err(e),
            Err(e) => e,
        };
        count(&STATS.fixes, 1);
        tracing::warn!("[{filename}] render failed, asking for a fix ({attempt}/{FIXES}): {err:#}");
        code = strip_fences(&ask(Tier::Mid, &[Part::Text(fix_prompt("", "", &code, &format!("{err:#}")))], false).await?).to_string();
    }
    unreachable!()
}

/// Compile errors, uncaught crashes, failing commands and bare usage messages, as opposed to
/// errors a program prints on purpose.
fn crashed(out: &str) -> bool {
    let markers = [": error:", "Exception in thread", "Traceback (most recent call last)", "Segmentation fault", "panicked at",
        "--help' for more information", ": command not found", ": syntax error"];
    markers.iter().any(|m| out.contains(m))
        || out.lines().any(|l| l.starts_with("error:") || l.contains(".sh: line "))
        || out.trim_start().to_lowercase().starts_with("usage:")
}

/// MySQL ends a statement at the first `;`, so a trigger, procedure or function body typed without
/// `DELIMITER` breaks apart. Wraps each such statement, from its `CREATE` to the `END;` closing it,
/// after undoing the model's own delimiters, which it sometimes gets half right (`$$` without `DELIMITER $$`).
fn with_delimiters(code: &str) -> String {
    let is_delimiter_cmd = |l: &str| l.trim().to_uppercase().starts_with("DELIMITER ");
    let delims: Vec<String> = code.lines().filter(|l| is_delimiter_cmd(l)).map(|l| l.trim()[10..].trim().to_string())
        .filter(|d| d != ";").chain(["$$".to_string()]).collect();
    let mut plain: Vec<String> = Vec::new();
    for line in code.lines().filter(|l| !is_delimiter_cmd(l)) {
        let line = line.trim_end();
        match delims.iter().find(|d| line.ends_with(d.as_str())) {
            Some(d) if line.trim() == d => {
                if let Some(prev) = plain.last_mut().filter(|p| !p.ends_with(';')) { prev.push(';') }
            }
            Some(d) => plain.push(format!("{};", line[..line.len() - d.len()].trim_end())),
            None => plain.push(line.to_string()),
        }
    }
    let lines: Vec<&str> = plain.iter().map(String::as_str).collect();
    // body_end: the line a routine ends on; usize::MAX for a BEGIN body, which ends at its END;
    let (mut out, mut body_end) = (Vec::new(), None);
    for (i, line) in lines.iter().enumerate() {
        let upper = line.trim().to_uppercase();
        let routine = upper.starts_with("CREATE ") && ["TRIGGER", "PROCEDURE", "FUNCTION"].iter().any(|k| upper.contains(k));
        // a body opens with BEGIN before the first `;`; one without (SET NEW.x = 1;) ends at that `;`
        let header_end = lines[i..].iter().position(|l| l.trim_end().ends_with(';')).map_or(lines.len(), |p| i + p + 1);
        if body_end.is_none() && routine {
            out.push("DELIMITER $$".to_string());
            let begin = lines[i..header_end].iter().any(|l| l.to_uppercase().contains("BEGIN"));
            body_end = Some(if begin { usize::MAX } else { header_end - 1 });
        }
        let ends = body_end.is_some_and(|e| if e == usize::MAX { upper == "END;" || upper.ends_with(" END;") } else { i == e });
        if ends {
            out.push(format!("{}$$", line.trim_end().trim_end_matches(';')));
            out.push("DELIMITER ;".to_string());
            body_end = None;
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

/// Models sometimes escape code twice, leaving literal `\n` in a one-line program.
fn unescape_code(code: &mut String) {
    if !code.contains('\n') && code.contains("\\n") {
        *code = code.replace("\\n", "\n").replace("\\t", "\t").replace("\\\"", "\"");
    }
}

/// True when every non-empty line of `code` appears in `out`.
fn shows_code(out: &str, code: &str) -> bool {
    let mut lines = code.lines().map(str::trim).filter(|l| !l.is_empty()).peekable();
    lines.peek().is_some() && lines.all(|l| out.contains(l))
}

/// Session code split at blank lines; each step becomes its own screenshot.
fn steps_of(code: &str) -> Vec<String> {
    code.split("\n\n").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Show the temp work dir as the user's home, like a real terminal would.
fn tidy_paths(out: &str, dir: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    let mut out = out.to_string();
    for path in [std::fs::canonicalize(dir).ok(), Some(dir.to_path_buf())].into_iter().flatten() {
        out = out.replace(&*path.to_string_lossy(), &home);
    }
    out
}

fn question_text(q: &Question) -> String {
    std::iter::once(q.text.as_str()).chain(q.parts.iter().map(String::as_str)).collect::<Vec<_>>().join("\n")
}

/// One label style for the whole document: the title's number ("Problem 2" → 2, "Q1(a)" → 1(a)),
/// or every whole title when any number repeats (a bonus section restarting at 1) or one has none.
fn assign_labels(groups: &mut [Group]) {
    let number = |title: &str| {
        let start = title.find(|c: char| c.is_ascii_digit())?;
        let rest = &title[start..];
        let len = rest.find(|c: char| !(c.is_ascii_digit() || c.is_ascii_lowercase() || "().".contains(c))).unwrap_or(rest.len());
        Some(rest[..len].trim_end_matches('.').to_string())
    };
    let numbers: Vec<Option<String>> = groups.iter().flat_map(|g| &g.questions).map(|q| number(&q.title)).collect();
    let unique: HashSet<&Option<String>> = numbers.iter().collect();
    let bare = unique.len() == numbers.len() && numbers.iter().all(Option::is_some);
    for (q, num) in groups.iter_mut().flat_map(|g| &mut g.questions).zip(numbers) {
        q.label = if bare { num.unwrap_or_default() } else { q.title.clone() };
    }
}

/// The model's dotted label on the question's own (`2.2` → `2(b)`, `2.1.3` → `2(a)(iii)`), keeping a
/// " (corrected)" suffix, so every document numbers its parts the same way. Without `split`, just the question's.
fn heading(base: &str, label: &str, split: bool) -> String {
    let (label, corrected) = label.strip_suffix(" (corrected)").map_or((label, ""), |l| (l, " (corrected)"));
    if !split { return format!("{base}{corrected}") }
    let parts: String = part_numbers(label).into_iter()
        .enumerate()
        .map(|(depth, n)| format!("({})", if depth == 0 { letter(n) } else { roman(n) }))
        .collect();
    format!("{base}{parts}{corrected}")
}

/// The part positions in a dotted label: `2.2.1 (corrected)` → [2, 1].
fn part_numbers(label: &str) -> Vec<usize> {
    let label = label.trim_end_matches(" (corrected)");
    let dotted = &label[label.find(|c: char| c.is_ascii_digit()).unwrap_or(label.len())..];
    dotted.split('.').skip(1).map_while(|p| p.trim().parse::<usize>().ok().filter(|&n| n > 0)).collect()
}

pub fn letter(n: usize) -> String {
    (b'a'..=b'z').nth(n - 1).map_or(n.to_string(), |c| (c as char).to_string())
}

pub fn roman(n: usize) -> String {
    ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii"].get(n - 1).map_or(n.to_string(), |r| r.to_string())
}

fn normalized(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// Where a question title first appears in the (normalized) document, so groups keep document order.
fn title_pos(doc: &str, title: &str) -> Option<usize> {
    let title = normalized(title);
    if title.is_empty() { return None }
    doc.match_indices(&title).map(|(i, _)| i)
        .find(|&i| !doc[i + title.len()..].starts_with(|c: char| c.is_ascii_digit() && title.ends_with(|t: char| t.is_ascii_digit())))
}

/// Fallback for unlabelled questions: the furthest point where a slice of the question text appears
/// exactly once. Furthest, because extracted text may open with shared context (a schema) quoted from earlier.
fn text_pos(doc: &str, text: &str) -> Option<usize> {
    let text = normalized(text);
    let width = text.len().min(40);
    (0..=text.len().saturating_sub(width)).step_by(10)
        .map(|i| &text[i..i + width])
        .filter(|slice| doc.matches(slice).count() == 1)
        .filter_map(|slice| doc.find(slice)).max()
}

/// `java File.java` needs the file named after its public class.
fn java_class(lang: &str, code: &str) -> Option<String> {
    let rest = code.split("public class ").nth(1).filter(|_| lang == "java")?;
    let class: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!class.is_empty()).then(|| format!("{class}.java"))
}

/// Consecutive short steps share one screenshot instead of one strip each.
fn merge_short_steps(steps: Vec<String>) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    for step in steps {
        match merged.last_mut() {
            Some(last) if last.lines().count() + step.lines().count() <= 20 => { last.push('\n'); last.push_str(&step); }
            _ => merged.push(step),
        }
    }
    merged
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
    let mut text = format!(
        "- file (saved as <label>.<lang> and run): {}\n\
         - session (typed into an interactive client line by line; state persists across the group): {}\n\
         - image (rendered to a picture): {}",
        lang_names(configs, |c| !c.session && !c.image),
        lang_names(configs, |c| c.session),
        lang_names(configs, |c| c.image),
    );
    let mut hints: Vec<(&String, &String)> = configs.iter().filter_map(|(name, c)| Some((name, c.hint.as_ref()?))).collect();
    hints.sort();
    for (name, hint) in hints { text.push_str(&format!("\n- {name}: {hint}")) }
    text.push_str(if crate::linux::enabled() {
        "\nCode runs as root in a throwaway Linux container: changing the system (crontab, services, packages) is fine."
    } else {
        "\nCode runs on the user's own machine: never change the system (no installing crontabs: show the line and run the script once instead; no sudo, services, package installs or deleting anything outside the working directory)."
    });
    text
}

/// Models put markdown in prose answers even when told not to; the docx shows it raw.
fn plain_text(text: &str) -> String {
    text.lines()
        .map(|l| l.trim_start_matches('#').trim_start().replace("**", "").replace("__", "").replace('`', ""))
        .collect::<Vec<_>>().join("\n").trim().to_string()
}

fn strip_fences(text: &str) -> &str {
    let t = text.trim();
    let t = t.strip_prefix("```").map_or(t, |rest| rest.split_once('\n').map_or(rest, |(_, body)| body));
    t.strip_suffix("```").unwrap_or(t).trim()
}

/// Ask for JSON, asking again (up to `--retries` times) when the reply doesn't parse.
async fn ask_json<T: DeserializeOwned>(tier: Tier, parts: &[Part]) -> Result<T> {
    for attempt in 0.. {
        let reply = ask(tier, parts, true).await?;
        tracing::debug!("AI reply: {reply}");
        match parse_json(&reply) {
            Err(e) if attempt < crate::ai::max_retries() => {
                count(&STATS.bad_json, 1);
                tracing::warn!("bad JSON from model, retry {}/{}: {e:#}", attempt + 1, crate::ai::max_retries());
            }
            result => return result,
        }
    }
    unreachable!()
}

/// Parse the JSON object or array in a model reply, ignoring markdown fences or chatter around it.
fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T> {
    let end = text.rfind(['}', ']']).context("no JSON in reply")?;
    let first = |json: &str| {
        let mut values = serde_json::Deserializer::from_str(json).into_iter::<T>();
        let value = values.next().context("no JSON in reply")??;
        // a value followed by `,` `]` `}` sits inside a bigger one (`"files":[]` reads as no answers)
        if json[values.byte_offset()..].trim_start().starts_with([',', ']', '}', ':']) { bail!("JSON nested in a reply that doesn't parse") }
        Ok(value)
    };
    let mut first_err = None;
    // reasoning before the JSON can mention brackets too, so try each opening one
    for (start, _) in text.match_indices(['{', '[']).take_while(|(i, _)| *i < end).take(30) {
        let json = &text[start..=end];
        match first(json).or_else(|e| first(&close_brackets(&fix_escapes(json))).map_err(|_| e)) {
            Ok(value) => return Ok(value),
            Err(e) => { first_err.get_or_insert(e); }
        }
    }
    Err(first_err.context("no JSON in reply")?).with_context(|| format!("bad JSON: {}", &text[..text.len().min(300)]))
}

/// Models copy shell escapes into JSON strings (`\$`, `\(`), which JSON rejects, and leave raw
/// newlines in them: keep such backslashes literally and escape the control characters.
fn fix_escapes(json: &str) -> String {
    let (mut out, mut in_str) = (String::with_capacity(json.len()), false);
    let mut chars = json.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => { in_str = !in_str; out.push(c) }
            '\\' => {
                out.push(c);
                match chars.next() {
                    Some(n @ ('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u')) => out.push(n),
                    Some(n) => { out.push('\\'); out.push(n) }
                    None => {}
                }
            }
            '\n' if in_str => out.push_str("\\n"),
            '\r' if in_str => out.push_str("\\r"),
            '\t' if in_str => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Models sometimes stop before the last closing brackets.
fn close_brackets(json: &str) -> String {
    let (mut open, mut in_str, mut escaped) = (Vec::new(), false, false);
    for c in json.chars() {
        match (in_str, c) {
            (true, _) if escaped => escaped = false,
            (true, '\\') => escaped = true,
            (true, '"') | (false, '"') => in_str = !in_str,
            (false, '{') => open.push('}'),
            (false, '[') => open.push(']'),
            (false, '}' | ']') => { open.pop(); }
            _ => {}
        }
    }
    json.chars().chain(open.into_iter().rev()).collect()
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
#[path = "tests/solve.rs"]
mod tests;
