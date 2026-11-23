//! Reports and synopses: one `Draft` (cover, questions, outline, written sections) goes from the
//! plan through writing and redos into the docx. The browser and the cli each keep it between steps.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::ai::{Part, Tier};
use crate::chart::Chart;
use crate::diagram::Diagram;
use crate::doc_theme::DocTheme;
use crate::editor_gen::Snippet;
use crate::prompts::{report_abstract_prompt, report_plan_prompt, report_write_prompt, AI_WORDS};
use crate::solve::ask_json;
use crate::term_gen::TermGenOptions;

/// Times a section is sent back with what's wrong with it before it's kept as it is.
const FIXES: usize = 2;
/// How much of the uploaded code the model sees, in characters.
const CODE_BUDGET: usize = 40_000;
const WORDS_PER_PAGE: usize = 330;

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Report,
    Synopsis,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Draft {
    pub kind: Kind,
    /// What the project is, in the user's words.
    pub idea: String,
    pub code_based: bool,
    pub pages: usize,
    /// A contents page after the cover.
    pub toc: bool,
    pub cover: Cover,
    /// Theme fields the format PDF asks for, over the chosen doc theme.
    pub look: Value,
    #[serde(deserialize_with = "skip_bad")]
    pub questions: Vec<Question>,
    #[serde(deserialize_with = "skip_bad")]
    pub runs: Vec<RunSpec>,
    #[serde(deserialize_with = "skip_bad")]
    pub sections: Vec<Section>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Cover {
    pub title: String,
    pub course: String,
    pub code: String,
    pub semester: String,
    pub members: Vec<Member>,
    pub supervisors: Vec<Supervisor>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Member {
    pub name: String,
    pub enrollment: String,
    /// What they did, for the team roles table.
    pub role: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Supervisor {
    /// Dr., Mr., Ms., Mrs. or Prof.
    pub honorific: String,
    pub name: String,
    pub designation: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Question {
    pub ask: String,
    #[serde(deserialize_with = "texts")]
    pub options: Vec<String>,
    /// What "assume" answers.
    #[serde(deserialize_with = "text")]
    pub default: String,
    pub answer: String,
}

/// One program run whose terminal screenshot a section shows.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RunSpec {
    pub id: String,
    /// An uploaded code file, relative to the code folder.
    pub file: String,
    #[serde(deserialize_with = "text")]
    pub stdin: String,
    #[serde(deserialize_with = "texts")]
    pub args: Vec<String>,
    /// What it printed, filled in after it runs.
    pub output: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Section {
    pub title: String,
    /// Abstract, references and appendices go without a number.
    pub numbered: bool,
    /// What the section should say.
    pub brief: String,
    #[serde(deserialize_with = "number")]
    pub words: usize,
    /// text, table, diagram, shot, snippet, chart or upload.
    #[serde(deserialize_with = "texts")]
    pub needs: Vec<String>,
    /// waiting, writing, checking, done or failed.
    pub state: String,
    #[serde(deserialize_with = "skip_bad")]
    pub blocks: Vec<Block>,
}

impl Default for Section {
    fn default() -> Self {
        Self { title: String::new(), numbered: true, brief: String::new(), words: 0, needs: vec![], state: "waiting".into(), blocks: vec![] }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    /// A sub-heading inside the section (numbered 2.1, 2.2, ...).
    #[serde(alias = "subheading", alias = "h2")]
    Heading { #[serde(deserialize_with = "text")] text: String },
    #[serde(alias = "paragraph", alias = "text", alias = "p")]
    Para { #[serde(deserialize_with = "text")] text: String },
    #[serde(alias = "list", alias = "bullet", alias = "bulleted", alias = "ul")]
    Bullets { #[serde(deserialize_with = "texts")] items: Vec<String> },
    #[serde(alias = "ordered", alias = "steps", alias = "ol")]
    Numbered { #[serde(deserialize_with = "texts")] items: Vec<String> },
    Table {
        #[serde(default, deserialize_with = "text")]
        caption: String,
        #[serde(default, deserialize_with = "texts")]
        header: Vec<String>,
        #[serde(default, deserialize_with = "rows")]
        rows: Vec<Vec<String>>,
    },
    #[serde(alias = "image", alias = "picture")]
    Figure(Figure),
}

/// A picture with a caption: exactly one of the sources is set.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Figure {
    #[serde(deserialize_with = "text")]
    pub caption: String,
    #[serde(deserialize_with = "lenient")]
    pub diagram: Option<Diagram>,
    /// A `RunSpec` id: its terminal screenshot.
    pub run: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub snippet: Option<Snippet>,
    #[serde(deserialize_with = "lenient")]
    pub chart: Option<Chart>,
    /// An uploaded image's file name.
    pub upload: Option<String>,
}

/// The uploads a report is written from, and where its run screenshots go.
pub struct Project {
    /// Format documents, code, screenshots, CSV data, and the logo under `logo/`.
    pub dir: PathBuf,
    pub work: PathBuf,
    pub theme: DocTheme,
    pub term: TermGenOptions,
    pub jobs: usize,
    code_exts: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Sort { Doc, Code, Image, Data, Logo, Other }

impl Project {
    pub fn new(dir: PathBuf, work: PathBuf, theme: DocTheme, term: TermGenOptions, jobs: usize) -> Result<Project> {
        let code_exts = crate::runner::load_configurations()?.into_iter().filter(|(_, c)| !c.image).map(|(ext, _)| ext).collect();
        std::fs::create_dir_all(work.join("shots"))?;
        Ok(Project { dir, work, theme, term, jobs, code_exts })
    }

    fn sort(&self, rel: &Path) -> Sort {
        if rel.starts_with("logo") { return Sort::Logo }
        let ext = rel.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        match ext.as_str() {
            "pdf" | "docx" | "txt" | "md" => Sort::Doc,
            "png" | "jpg" | "jpeg" | "webp" => Sort::Image,
            "csv" | "tsv" => Sort::Data,
            e if self.code_exts.iter().any(|c| c == e) => Sort::Code,
            _ => Sort::Other,
        }
    }

    /// Uploaded files of one sort, relative to `dir`, in natural order.
    fn files(&self, sort: Sort) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = ignore::WalkBuilder::new(&self.dir).build().flatten()
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .filter_map(|e| e.path().strip_prefix(&self.dir).ok().map(Path::to_path_buf))
            .filter(|rel| self.sort(rel) == sort)
            .collect();
        files.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));
        files
    }

    /// An uploaded file a model names, by its path or just its file name; nothing outside the uploads.
    fn find(&self, sort: Sort, name: &str) -> Option<PathBuf> {
        let name = name.trim().trim_start_matches("./");
        let files = self.files(sort);
        files.iter().find(|f| f.to_string_lossy() == name)
            .or_else(|| files.iter().find(|f| f.file_name().is_some_and(|n| n.to_string_lossy() == name)))
            .map(|f| self.dir.join(f))
    }

    pub fn logo(&self) -> Option<Vec<u8>> {
        self.files(Sort::Logo).first().and_then(|f| std::fs::read(self.dir.join(f)).ok())
    }

    fn shot_path(&self, id: &str) -> PathBuf {
        let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        self.work.join("shots").join(format!("{id}.png"))
    }

    /// Everything the models know about the project, the same for every call of a run.
    fn summary(&self, draft: &Draft) -> String {
        let c = &draft.cover;
        let mut out = format!("Project title: {}\nIdea: {}\n", c.title, draft.idea);
        for (label, value) in [("Course", &c.course), ("Course code", &c.code), ("Semester", &c.semester)] {
            if !value.is_empty() { out.push_str(&format!("{label}: {value}\n")) }
        }
        let members: Vec<String> = c.members.iter().filter(|m| !m.name.is_empty())
            .map(|m| if m.role.is_empty() { m.name.clone() } else { format!("{} (did: {})", m.name, m.role) }).collect();
        if !members.is_empty() { out.push_str(&format!("Group members: {}\n", members.join(", "))) }
        let answered: Vec<String> = draft.questions.iter().map(|q| {
            let answer = if q.answer.trim().is_empty() { format!("{} (assumed)", q.default) } else { q.answer.clone() };
            format!("- {} {answer}", q.ask)
        }).collect();
        if !answered.is_empty() { out.push_str(&format!("\nAnswers from the student:\n{}\n", answered.join("\n"))) }

        let code = self.files(Sort::Code);
        if !code.is_empty() {
            out.push_str("\nUploaded code (line numbers before |):\n");
            let mut budget = CODE_BUDGET;
            for rel in &code {
                let text = std::fs::read_to_string(self.dir.join(rel)).unwrap_or_default();
                if text.len() > budget {
                    out.push_str(&format!("--- {} ({} lines, not shown) ---\n", rel.display(), text.lines().count()));
                    continue;
                }
                budget -= text.len();
                let numbered: String = text.lines().enumerate().map(|(i, l)| format!("{:>4}| {l}\n", i + 1)).collect();
                out.push_str(&format!("--- {} ---\n{numbered}", rel.display()));
            }
        }
        for rel in self.files(Sort::Data) {
            let text = std::fs::read_to_string(self.dir.join(&rel)).unwrap_or_default();
            let rows = text.lines().filter(|l| !l.trim().is_empty()).count().saturating_sub(1);
            let head: Vec<&str> = text.lines().take(4).collect();
            out.push_str(&format!("\nUploaded data {} ({rows} rows), first lines:\n{}\n", rel.display(), head.join("\n")));
        }
        let images: Vec<String> = self.files(Sort::Image).iter().map(|f| f.display().to_string()).collect();
        if !images.is_empty() { out.push_str(&format!("\nUploaded images: {}\n", images.join(", "))) }
        let runs: Vec<String> = draft.runs.iter().filter(|r| !r.output.is_empty()).map(|r| {
            let output: String = r.output.chars().take(2500).collect();
            format!("- {}: {} with input {:?}, printed:\n{output}", r.id, r.file, r.stdin)
        }).collect();
        if runs.is_empty() { out.push_str("\nRuns: none, so no terminal screenshots can be shown.\n") }
        else { out.push_str(&format!("\nRuns (their screenshots can be shown):\n{}\n", runs.join("\n"))) }
        out
    }

    /// The PNG a figure shows.
    fn picture(&self, f: &Figure) -> Result<Vec<u8>> {
        if let Some(d) = &f.diagram { return d.render() }
        if let Some(id) = &f.run { return std::fs::read(self.shot_path(id)).with_context(|| format!("no screenshot of run {id}")) }
        if let Some(s) = &f.snippet {
            let path = self.find(Sort::Code, &s.file).with_context(|| format!("no uploaded code file {}", s.file))?;
            let text = std::fs::read_to_string(&path)?;
            let lines: Vec<&str> = text.lines().collect();
            let from = s.from.clamp(1, lines.len().max(1));
            let to = s.to.clamp(from, lines.len().max(from));
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            return crate::editor_gen::render(&lines[from - 1..to.min(lines.len())], from, ext);
        }
        if let Some(c) = &f.chart { return c.render(&self.data_dir(c)) }
        if let Some(name) = &f.upload { return std::fs::read(self.find(Sort::Image, name).with_context(|| format!("no uploaded image {name}"))?).map_err(Into::into) }
        bail!("the figure shows nothing")
    }

    fn data_dir(&self, c: &Chart) -> PathBuf {
        self.find(Sort::Data, &c.csv).and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| self.dir.clone())
    }

    /// What's wrong with a written section, in words a model can act on.
    fn problems(&self, s: &Section, runs: &[RunSpec]) -> Vec<String> {
        if s.blocks.is_empty() { return vec!["it is empty".into()] }
        let mut out = Vec::new();
        let text = s.blocks.iter().map(block_text).collect::<Vec<_>>().join(" ").to_lowercase();
        let words = text.split_whitespace().count();
        if s.words > 0 && words < s.words / 2 { out.push(format!("it has {words} words; write about {}", s.words)) }
        let used: Vec<&str> = AI_WORDS.iter().copied().filter(|w| uses(&text, w)).collect();
        if !used.is_empty() { out.push(format!("rewrite without: {}", used.join(", "))) }
        let figures: Vec<&Figure> = s.blocks.iter().filter_map(|b| if let Block::Figure(f) = b { Some(f) } else { None }).collect();
        let has = |want: fn(&Figure) -> bool| figures.iter().any(|f| want(f));
        for need in &s.needs {
            let missing = match need.as_str() {
                "table" => !s.blocks.iter().any(|b| matches!(b, Block::Table { .. })),
                "diagram" => !has(|f| f.diagram.is_some()),
                "shot" => !runs.is_empty() && !has(|f| f.run.is_some()),
                "snippet" => !self.files(Sort::Code).is_empty() && !has(|f| f.snippet.is_some()),
                "chart" => !has(|f| f.chart.is_some()),
                "upload" => !self.files(Sort::Image).is_empty() && !has(|f| f.upload.is_some()),
                _ => false,
            };
            if missing { out.push(format!("it needs a {need}")) }
        }
        for f in figures {
            let sources = [f.diagram.is_some(), f.run.is_some(), f.snippet.is_some(), f.chart.is_some(), f.upload.is_some()].iter().filter(|x| **x).count();
            if sources != 1 { out.push(format!("figure \"{}\" needs exactly one of diagram, run, snippet, chart or upload", f.caption)); continue }
            if let Some(d) = &f.diagram { out.extend(d.problems().into_iter().map(|p| format!("figure \"{}\": {p}", f.caption))) }
            if let Some(id) = &f.run {
                if !runs.iter().any(|r| &r.id == id) { out.push(format!("figure \"{}\" shows run \"{id}\", but the runs are: {}", f.caption, runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join(", "))) }
            }
            if let Some(c) = &f.chart {
                if let Err(e) = c.data(&self.data_dir(c)) { out.push(format!("figure \"{}\": {e:#}", f.caption)) }
            }
            if f.snippet.is_some() || f.upload.is_some() {
                if let Err(e) = self.picture(f) { out.push(format!("figure \"{}\": {e:#}", f.caption)) }
            }
        }
        out
    }
}

/// Whether `text` uses an AI word, in any form of it (utilize, utilized, utilizing).
fn uses(text: &str, word: &str) -> bool {
    if word.contains(' ') || word.contains('\'') { return text.contains(word) }
    let stem = if word.len() > 4 { word.trim_end_matches('e') } else { word };
    text.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| w.starts_with(stem))
}

fn block_text(b: &Block) -> String {
    match b {
        Block::Heading { text } | Block::Para { text } => text.clone(),
        Block::Bullets { items } | Block::Numbered { items } => items.join(" "),
        Block::Table { caption, header, rows } => format!("{caption} {} {}", header.join(" "), rows.iter().map(|r| r.join(" ")).collect::<Vec<_>>().join(" ")),
        Block::Figure(f) => f.caption.clone(),
    }
}

/// Em dashes are the surest sign of AI text; commas read the same.
fn tidy(blocks: &mut [Block]) {
    let fix = |t: &mut String| *t = t.replace(" — ", ", ").replace(" —", ",").replace("—", ", ");
    for b in blocks {
        match b {
            Block::Heading { text } | Block::Para { text } => fix(text),
            Block::Bullets { items } | Block::Numbered { items } => items.iter_mut().for_each(fix),
            Block::Table { caption, header, rows } => { fix(caption); header.iter_mut().chain(rows.iter_mut().flatten()).for_each(fix) }
            Block::Figure(f) => fix(&mut f.caption),
        }
    }
}

fn is_abstract(s: &Section) -> bool {
    let t = s.title.to_lowercase();
    t.contains("abstract") || t.contains("executive summary")
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Plan {
    course: String,
    code: String,
    semester: String,
    code_based: Option<bool>,
    #[serde(deserialize_with = "number")]
    pages: usize,
    toc: bool,
    look: Value,
    #[serde(deserialize_with = "skip_bad")]
    sections: Vec<Section>,
    #[serde(deserialize_with = "skip_bad")]
    runs: Vec<RunSpec>,
    #[serde(deserialize_with = "skip_bad")]
    questions: Vec<Question>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Written {
    #[serde(deserialize_with = "skip_bad")]
    sections: Vec<Section>,
    #[serde(deserialize_with = "skip_bad")]
    blocks: Vec<Block>,
}

fn kind_name(draft: &Draft) -> &'static str {
    match (draft.kind, draft.code_based) { (Kind::Synopsis, _) => "synopsis", (_, true) => "report (code-based)", _ => "report" }
}

/// Step 1: one SMART call reads the format document, the code and the idea, and fills in the cover,
/// the outline, the runs and the questions. What the user already typed on the cover stays.
pub async fn plan(p: &Project, draft: &mut Draft) -> Result<()> {
    let docs: Vec<PathBuf> = p.files(Sort::Doc).iter().map(|f| p.dir.join(f)).collect();
    let read = if docs.is_empty() { None } else { Some(crate::solve::read_inputs(&docs, 60)?) };
    let photos = read.as_ref().is_some_and(|r| !r.images.is_empty()) && crate::ai::sees_images(Tier::Smart);
    let format = read.as_ref().map_or("(none given: use the default sections)".to_string(), |r| r.text.clone());
    let kind = if draft.kind == Kind::Synopsis { "synopsis" } else { "report" };
    let mut parts = vec![Part::Text(report_plan_prompt(kind, &p.summary(draft), &format, photos))];
    if let Some(read) = read.filter(|_| photos) { parts.extend(read.images) }
    let plan: Plan = ask_json(Tier::Smart, &parts).await.context("planning the outline")?;

    let cover = &mut draft.cover;
    for (mine, found) in [(&mut cover.course, plan.course), (&mut cover.code, plan.code), (&mut cover.semester, plan.semester)] {
        if mine.trim().is_empty() { *mine = found.trim().to_string() }
    }
    draft.code_based = plan.code_based.unwrap_or(!p.files(Sort::Code).is_empty());
    draft.pages = if plan.pages > 0 { plan.pages } else if draft.kind == Kind::Synopsis { 3 } else { 12 };
    draft.toc = plan.toc;
    draft.look = plan.look;
    draft.runs = plan.runs.into_iter().filter(|r| p.find(Sort::Code, &r.file).is_some()).enumerate()
        .map(|(i, r)| RunSpec { id: if r.id.trim().is_empty() { format!("run{}", i + 1) } else { r.id.trim().to_string() }, output: String::new(), ..r })
        .collect();
    // the cover has the title, and a section can't show what there's nothing of
    let has = [("shot", !draft.runs.is_empty()), ("snippet", !p.files(Sort::Code).is_empty()), ("upload", !p.files(Sort::Image).is_empty())];
    draft.sections = plan.sections.into_iter()
        .filter(|s| !s.title.trim().is_empty() && !["title", "project title", "title of the project", "title of project"].contains(&s.title.trim().to_lowercase().as_str()))
        .map(|mut s| {
            s.needs.retain(|n| has.iter().all(|(what, ok)| n != what || *ok));
            Section { state: "waiting".into(), blocks: vec![], ..s }
        })
        .collect();
    let share = (draft.pages * WORDS_PER_PAGE / draft.sections.len().max(1)).max(60);
    for s in &mut draft.sections { if s.words == 0 { s.words = share } }
    draft.questions = plan.questions.into_iter().filter(|q| !q.ask.trim().is_empty()).map(|mut q| {
        if q.default.trim().is_empty() { q.default = q.options.first().cloned().unwrap_or_default() }
        q.answer.clear();
        q
    }).collect();
    if draft.sections.is_empty() { bail!("the plan had no sections") }
    Ok(())
}

/// Runs each program the plan picked, once; its output feeds the writer and its screenshot a figure.
async fn run_all(p: &Project, draft: &mut Draft) {
    for run in draft.runs.iter_mut().filter(|r| r.output.is_empty()) {
        let Some(path) = p.find(Sort::Code, &run.file) else { continue };
        let folder = path.parent().unwrap_or(&p.dir).to_path_buf();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let (cmd, out, _) = crate::pack::run_with_input(&path, &folder, &name, &content, Some(&run.stdin), &run.args, crate::pack::AiInput::Never, None).await;
        run.output = crate::runner::final_screen(&out);
        match crate::pack::screenshot(Some(&cmd), &out, &p.term, true) {
            Ok(shot) => if let Err(e) = std::fs::write(p.shot_path(&run.id), shot.bytes) { tracing::warn!("saving the {} screenshot: {e}", run.id) },
            Err(e) => tracing::warn!("screenshot of {} failed: {e:#}", run.id),
        }
    }
}

/// The outline as the writer sees it.
fn outline(draft: &Draft) -> String {
    draft.sections.iter().enumerate().map(|(i, s)| format!("{}. {}: {}", i + 1, s.title, s.brief)).collect::<Vec<_>>().join("\n")
}

fn asked(sections: &[Section]) -> String {
    sections.iter().map(|s| {
        let needs: Vec<&str> = s.needs.iter().map(String::as_str).filter(|n| *n != "text").collect();
        let needs = if needs.is_empty() { String::new() } else { format!(" Needs: {}.", needs.join(", ")) };
        format!("- {}: {} About {} words.{needs}", s.title, s.brief, s.words)
    }).collect::<Vec<_>>().join("\n")
}

/// Writes `targets` in one MID call, sending them back with their problems up to `FIXES` times.
async fn write_sections(p: &Project, draft: &Draft, project: &str, targets: Vec<Section>, change: Option<&str>) -> Result<Vec<Section>> {
    let mut prompt = report_write_prompt(kind_name(draft), project, &outline(draft), &asked(&targets));
    if let Some(note) = change {
        prompt.push_str(&format!("\n\nThe student wants this section changed: {note}\nIts current version:\n{}", serde_json::to_string(&targets[0].blocks)?));
    }
    let mut parts = vec![Part::Text(prompt)];
    for attempt in 0..=FIXES {
        let reply: Written = ask_json(Tier::Mid, &parts).await?;
        let mut written = targets.clone();
        // a one-section reply sometimes comes as the section itself
        let sections = if reply.sections.is_empty() && targets.len() == 1 { vec![Section { blocks: reply.blocks, ..Default::default() }] } else { reply.sections };
        let mut replies = sections.into_iter();
        for s in &mut written {
            s.blocks = replies.next().map(|r| r.blocks).unwrap_or_default();
            tidy(&mut s.blocks);
        }
        let problems: Vec<String> = written.iter().flat_map(|s| p.problems(s, &draft.runs).into_iter().map(move |e| format!("{}: {e}", s.title))).collect();
        if problems.is_empty() || attempt == FIXES {
            if !problems.is_empty() {
                tracing::warn!("kept with problems: {}", problems.join("; "));
                for s in &mut written { s.blocks.retain(|b| !matches!(b, Block::Figure(f) if p.picture(f).is_err())) }
            }
            for s in &mut written { s.state = if s.blocks.is_empty() { "failed" } else { "done" }.into() }
            return Ok(written);
        }
        tracing::info!("asking again ({}/{FIXES}): {}", attempt + 1, problems.join("; "));
        let mine = serde_json::to_string(&written.iter().map(|s| serde_json::json!({ "title": s.title, "blocks": s.blocks })).collect::<Vec<_>>())?;
        parts.truncate(1);
        parts.push(Part::Text(format!("\n\nYour last reply:\n{{\"sections\":{mine}}}\n\nFix these problems and reply again with the whole JSON:\n- {}", problems.join("\n- "))));
    }
    unreachable!()
}

async fn write_abstract(draft: &Draft, project: &str, target: &Section) -> Result<Section> {
    let written: String = draft.sections.iter().filter(|s| !is_abstract(s) && s.state == "done")
        .map(|s| format!("## {}\n{}\n", s.title, s.blocks.iter().map(block_text).collect::<Vec<_>>().join("\n"))).collect();
    let words = if target.words > 0 { target.words.min(300) } else { 180 };
    let reply: Written = ask_json(Tier::Mid, &[Part::Text(report_abstract_prompt(kind_name(draft), project, &written, words))]).await?;
    let mut s = target.clone();
    s.blocks = reply.blocks;
    tidy(&mut s.blocks);
    s.state = if s.blocks.is_empty() { "failed" } else { "done" }.into();
    Ok(s)
}

/// Steps 3 and 4: runs the programs, then writes every section not done yet (a report one per call,
/// a synopsis three per call, all at once up to `jobs`), then the abstract from what was written.
/// `update` sees the draft after every change.
pub async fn write(p: &Project, draft: &mut Draft, update: &(dyn Fn(&Draft) + Send + Sync)) -> Result<()> {
    run_all(p, draft).await;
    let todo: Vec<usize> = (0..draft.sections.len()).filter(|&i| draft.sections[i].state != "done").collect();
    for &i in &todo { draft.sections[i].state = "writing".into() }
    update(draft);
    let (body, abstracts): (Vec<usize>, Vec<usize>) = todo.into_iter().partition(|&i| !is_abstract(&draft.sections[i]));
    let per_call = if draft.kind == Kind::Synopsis { 3 } else { 1 };
    let project = p.summary(draft);
    let snapshot = draft.clone();
    let sem = tokio::sync::Semaphore::new(p.jobs.max(1));
    let mut pending: FuturesUnordered<_> = body.chunks(per_call).map(|chunk| {
        let targets: Vec<Section> = chunk.iter().map(|&i| snapshot.sections[i].clone()).collect();
        let (sem, snapshot, project) = (&sem, &snapshot, &project);
        async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            (chunk, write_sections(p, snapshot, project, targets, None).await)
        }
    }).collect();
    while let Some((chunk, result)) = pending.next().await {
        match result {
            Ok(written) => for (&i, s) in chunk.iter().zip(written) { draft.sections[i] = s },
            Err(e) => {
                tracing::warn!("writing {} failed: {e:#}", chunk.iter().map(|&i| draft.sections[i].title.as_str()).collect::<Vec<_>>().join(", "));
                for &i in chunk { draft.sections[i].state = "failed".into() }
            }
        }
        update(draft);
    }
    drop(pending);
    for i in abstracts {
        draft.sections[i] = write_abstract(draft, &project, &draft.sections[i]).await.unwrap_or_else(|e| {
            tracing::warn!("writing the abstract failed: {e:#}");
            Section { state: "failed".into(), ..draft.sections[i].clone() }
        });
        update(draft);
    }
    Ok(())
}

/// Step 7: writes one section again with the student's note.
pub async fn redo(p: &Project, draft: &mut Draft, index: usize, note: &str) -> Result<()> {
    let target = draft.sections.get(index).context("no such section")?.clone();
    let project = p.summary(draft);
    draft.sections[index] = if is_abstract(&target) {
        write_abstract(draft, &project, &target).await?
    } else {
        let note = if note.trim().is_empty() { "write it better" } else { note.trim() };
        write_sections(p, draft, &project, vec![target], Some(note)).await?.remove(0)
    };
    Ok(())
}

/// Copies the inputs (files, or folders without their gitignored files) into `dir`, and the logo into `dir/logo`.
pub fn gather(inputs: &[PathBuf], logo: Option<&Path>, dir: &Path) -> Result<()> {
    let copy = |from: &Path, to: PathBuf| -> Result<()> {
        if let Some(parent) = to.parent() { std::fs::create_dir_all(parent)? }
        std::fs::copy(from, &to).with_context(|| format!("can't read {}", from.display()))?;
        Ok(())
    };
    for input in inputs {
        let name = input.file_name().context("bad input path")?;
        if !input.is_dir() { copy(input, dir.join(name))?; continue }
        for entry in ignore::WalkBuilder::new(input).build().flatten().filter(|e| e.file_type().is_some_and(|t| t.is_file())) {
            copy(entry.path(), dir.join(name).join(entry.path().strip_prefix(input)?))?;
        }
    }
    if let Some(logo) = logo { copy(logo, dir.join("logo").join(logo.file_name().context("bad logo path")?))? }
    Ok(())
}

/// The docx of the draft as it stands, written or not.
pub fn build(p: &Project, draft: &Draft, out: &Path) -> Result<()> {
    crate::report_docx::generate(out, draft, &p.theme, p.logo().as_deref(), &|f| p.picture(f))
}

/// Models write numbers and booleans where text is asked for.
fn as_text(v: Value) -> String {
    match v {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A count, also when a model writes it as text ("150") or a range ("150-200").
fn number<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64().unwrap_or(0.0).max(0.0) as usize,
        Value::String(s) => s.split(|c: char| !c.is_ascii_digit()).find(|p| !p.is_empty()).and_then(|p| p.parse().ok()).unwrap_or(0),
        _ => 0,
    })
}

fn text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(as_text(Value::deserialize(d)?))
}

fn texts<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(items) => items.into_iter().map(as_text).collect(),
        Value::Null => vec![],
        one => vec![as_text(one)],
    })
}

fn rows<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<String>>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(rows) => rows.into_iter().map(|r| match r {
            Value::Array(cells) => cells.into_iter().map(as_text).collect(),
            one => vec![as_text(one)],
        }).collect(),
        _ => vec![],
    })
}

/// A list where one malformed item is dropped instead of failing the whole reply.
fn skip_bad<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(items) => items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect(),
        _ => vec![],
    })
}

fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sloppy_model_json_still_reads() {
        let section: Section = serde_json::from_str(r#"{"title":"Results","blocks":[
            {"type":"para","text":"ok"},
            {"type":"table","caption":"Tests","header":["Input",2],"rows":[["a",1],"b"]},
            {"type":"mystery"},
            {"type":"figure","caption":"Flow","diagram":"not an object"}
        ]}"#).unwrap();
        assert!(section.numbered && section.state == "waiting");
        assert_eq!(section.blocks.len(), 3);
        let Block::Table { header, rows, .. } = &section.blocks[1] else { panic!() };
        assert_eq!((header.clone(), rows.clone()), (vec!["Input".into(), "2".into()], vec![vec!["a".to_string(), "1".into()], vec!["b".into()]]));
        let Block::Figure(f) = &section.blocks[2] else { panic!() };
        assert!(f.diagram.is_none() && f.caption == "Flow");
    }

    #[test]
    fn other_names_for_blocks_and_numbers_as_text() {
        let s: Section = serde_json::from_str(r#"{"title":"x","words":"150-200","blocks":[{"type":"paragraph","text":"a"},{"type":"list","items":["b"]},{"type":"image","caption":"c","upload":"u.png"}]}"#).unwrap();
        assert_eq!((s.words, s.blocks.len()), (150, 3));
        assert!(matches!(s.blocks[1], Block::Bullets { .. }) && matches!(s.blocks[2], Block::Figure(_)));
    }

    #[test]
    fn ai_words_in_any_form() {
        assert!(uses("we utilized java", "utilize") && uses("a robust, simple tool", "robust") && uses("in today's world", "in today's"));
        assert!(!uses("the user enters data", "utilize") && !uses("we chose", "elevate"));
    }

    #[test]
    fn em_dashes_become_commas() {
        let mut blocks = vec![Block::Para { text: "fast — and small—cheap".into() }];
        tidy(&mut blocks);
        let Block::Para { text } = &blocks[0] else { panic!() };
        assert_eq!(text, "fast, and small, cheap");
    }

    #[test]
    fn checks_catch_what_models_get_wrong() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.c"), "int main() {\n  return 0;\n}\n").unwrap();
        let p = Project::new(dir.path().into(), dir.path().join("work"), DocTheme::default(), TermGenOptions::default(), 1).unwrap();
        let s: Section = serde_json::from_str(r#"{"title":"Design","words":40,"needs":["diagram","snippet"],"blocks":[
            {"type":"para","text":"We leverage a robust design."},
            {"type":"figure","caption":"Code","snippet":{"file":"nope.c","from":1,"to":2}},
            {"type":"figure","caption":"Run","run":"run9"}]}"#).unwrap();
        let problems = p.problems(&s, &[RunSpec { id: "run1".into(), ..Default::default() }]).join(" | ");
        for want in ["words", "leverage", "needs a diagram", "no uploaded code file nope.c", "run9"] { assert!(problems.contains(want), "{want}: {problems}") }
        let good: Section = serde_json::from_str(r#"{"title":"Code","needs":["snippet"],"blocks":[{"type":"figure","caption":"Main","snippet":{"file":"main.c","from":1,"to":3}}]}"#).unwrap();
        assert!(p.problems(&good, &[]).is_empty(), "{:?}", p.problems(&good, &[]));
    }
}
