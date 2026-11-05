use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

use crate::prompts::input_generation_prompt;

/// One piece of a multimodal prompt: text, or an image as (mime type, bytes).
pub enum Part {
    Text(String),
    Image(&'static str, Vec<u8>),
}

fn gemini_parts(parts: &[Part]) -> Vec<Value> {
    use base64::Engine;
    parts.iter().map(|p| match p {
        Part::Text(t) => json!({"text": t}),
        Part::Image(mime, bytes) => json!({"inline_data": {"mime_type": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes)}}),
    }).collect()
}

fn openai_parts(parts: &[Part]) -> Vec<Value> {
    use base64::Engine;
    parts.iter().map(|p| match p {
        Part::Text(t) => json!({"type": "text", "text": t}),
        Part::Image(mime, bytes) => json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", mime, base64::engine::general_purpose::STANDARD.encode(bytes))}}),
    }).collect()
}

/// SMART extracts questions (once per solve), MID solves them, FAST writes stdin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tier {
    Smart,
    Mid,
    Fast,
}

impl Tier {
    fn name(self) -> &'static str {
        ["SMART", "MID", "FAST"][self as usize]
    }

    /// This tier, then the ones it falls back to when unset.
    fn chain(self) -> &'static [Tier] {
        match self {
            Tier::Smart => &[Tier::Smart],
            Tier::Mid => &[Tier::Mid, Tier::Smart],
            Tier::Fast => &[Tier::Fast, Tier::Mid, Tier::Smart],
        }
    }
}

/// A coding agent CLI run in print mode. Empty model = the CLI's own default.
#[derive(Debug)]
pub struct Agent {
    name: &'static str,
    bin: &'static str,
    models: [&'static str; 3],
}

/// Also the auto-detect order.
const AGENTS: &[Agent] = &[
    Agent { name: "claude", bin: "claude", models: ["opus", "sonnet", "haiku"] },
    Agent { name: "codex", bin: "codex", models: ["", "", ""] },
    Agent { name: "gemini-cli", bin: "gemini", models: ["pro", "flash", "flash-lite"] },
    Agent { name: "cursor", bin: "cursor-agent", models: ["", "", ""] },
    Agent { name: "opencode", bin: "opencode", models: ["", "", ""] },
    Agent { name: "copilot", bin: "copilot", models: ["", "", ""] },
    Agent { name: "qwen", bin: "qwen", models: ["", "", ""] },
    Agent { name: "amp", bin: "amp", models: ["", "", ""] },
    Agent { name: "antigravity", bin: "agy", models: ["gemini-3.8-flash-high", "gemini-3.8-flash-medium", "gemini-3.8-flash-low"] },
];

const GEMINI_MODELS: [&str; 3] = [
    "gemini-3.6-flash,gemini-3.5-flash",
    "gemini-3.5-flash,gemini-3.5-flash-lite",
    "gemini-3.5-flash-lite,gemini-3.1-flash-lite",
];

const AGENT_TIMEOUT: Duration = Duration::from_secs(600);
/// Starting many agent processes at once trips server-side burst limits.
const AGENT_START_GAP: Duration = Duration::from_millis(300);
const DEFAULT_RETRIES: usize = 3;

#[derive(Clone, Debug)]
pub enum Provider {
    Gemini,
    OpenAi,
    Ollama,
    Agent(&'static Agent, PathBuf),
}

#[derive(Clone, Debug)]
pub struct AiConfig {
    pub provider: Provider,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: String,
    /// `{TIER}_THINKING`: off, low, medium, high, xhigh. None leaves the provider's default.
    pub thinking: Option<String>,
    tier: Tier,
    /// Picked by auto-detect, so another detected agent may take over when it fails.
    auto: bool,
}

static OVERRIDES: OnceLock<HashMap<String, String>> = OnceLock::new();
static DEAD_AGENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
/// How often each model actually answered, since a routing gateway (`auto`) picks one per request.
static ANSWERED_BY: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());
static AGENT_SLOTS: OnceLock<Vec<Arc<Semaphore>>> = OnceLock::new();
static LAST_AGENT_START: OnceLock<tokio::sync::Mutex<Instant>> = OnceLock::new();

/// Values from CLI flags (e.g. `SMART_PROVIDER`), checked before the environment.
pub fn set_overrides(values: HashMap<String, String>) {
    let _ = OVERRIDES.set(values);
}

fn var(key: &str) -> Option<String> {
    OVERRIDES.get().and_then(|o| o.get(key).cloned())
        .or_else(|| std::env::var(key).ok())
        .filter(|v| !v.trim().is_empty())
}

fn find_bin(bin: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ""] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| exts.iter().map(move |ext| dir.join(format!("{bin}{ext}"))))
        .find(|p| p.is_file())
}

/// `-p` help: every provider, with the agent CLIs found on this machine first.
pub fn provider_help() -> String {
    let (found, missing): (Vec<_>, Vec<_>) = AGENTS.iter().partition(|a| find_bin(a.bin).is_some());
    let names = |agents: Vec<&Agent>| agents.iter().map(|a| if a.name == a.bin { a.name.to_string() } else { format!("{} ({})", a.name, a.bin) }).collect::<Vec<_>>().join(", ");
    let found = if found.is_empty() { "none".to_string() } else { names(found) };
    format!("Provider for SMART: extracting questions in `solve` (once per run). MID and FAST fall back to it, so `-p codex` runs everything on one provider.\n\
        APIs: gemini, openai, ollama. Agents installed here: {found}. Not installed: {}", names(missing))
}

impl AiConfig {
    /// A Gemini API config with a user-supplied key (the web UI).
    pub fn gemini(api_key: String, tier: Tier) -> Self {
        Self { provider: Provider::Gemini, model: GEMINI_MODELS[tier as usize].into(), base_url: None, api_key, thinking: None, tier, auto: false }
    }

    /// Agents keep FAST cheap unless thinking is set.
    fn effort(&self) -> Option<&str> {
        self.thinking.as_deref().or((self.tier == Tier::Fast).then_some("low"))
    }

    fn label(&self) -> String {
        let name = match &self.provider {
            Provider::Gemini => "gemini",
            Provider::OpenAi => "openai",
            Provider::Ollama => "ollama",
            Provider::Agent(a, _) => a.name,
        };
        let model = if self.model.is_empty() { "auto" } else { &self.model };
        format!("{name} ({model}){}", if self.auto { ", auto-detected" } else { "" })
    }

    async fn send(&self, parts: &[Part], json_only: bool) -> Result<String> {
        match &self.provider {
            Provider::Gemini => gemini(self, parts, json_only).await,
            Provider::OpenAi => openai(self, parts).await,
            Provider::Ollama => ollama(self, parts).await,
            Provider::Agent(agent, bin) => run_agent(agent, bin, &self.model, self.effort(), parts).await,
        }
    }
}

/// Configs to try for `tier`: CLI flags / env first, then every agent CLI on PATH, then `GEMINI_API_KEY`.
pub fn resolve(tier: Tier) -> Result<Vec<AiConfig>> {
    resolve_with(tier, &var, &find_bin)
}

fn resolve_with(tier: Tier, var: &dyn Fn(&str) -> Option<String>, find_bin: &dyn Fn(&str) -> Option<PathBuf>) -> Result<Vec<AiConfig>> {
    let get = |from: Tier, key: &str| var(&format!("{}_{key}", from.name()));
    let thinking = tier.chain().iter().find_map(|t| get(*t, "THINKING"));
    if let Some(depth) = tier.chain().iter().position(|t| get(*t, "PROVIDER").is_some()) {
        let from = tier.chain()[depth];
        let name = get(from, "PROVIDER").unwrap();
        // the tier's own model wins, even when the provider comes from a tier above
        let model = tier.chain()[..=depth].iter().find_map(|t| get(*t, "MODEL"));
        let (provider, default_model) = match name.as_str() {
            "gemini" => (Provider::Gemini, GEMINI_MODELS[tier as usize]),
            "openai" => (Provider::OpenAi, ""),
            "ollama" => (Provider::Ollama, ""),
            _ => {
                let agent = AGENTS.iter().find(|a| a.name == name).with_context(|| format!(
                    "unknown {}_PROVIDER '{name}' (use gemini, openai, ollama, {})", from.name(),
                    AGENTS.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")))?;
                let bin = find_bin(agent.bin).with_context(|| format!("{} not found on PATH", agent.bin))?;
                (Provider::Agent(agent, bin), agent.models[tier as usize])
            }
        };
        let api_key = match provider {
            Provider::Gemini => get(from, "API_KEY").or_else(|| var("GEMINI_API_KEY"))
                .with_context(|| format!("{}_API_KEY or GEMINI_API_KEY must be set for gemini", from.name()))?,
            Provider::OpenAi => get(from, "API_KEY").with_context(|| format!("{}_API_KEY must be set for openai", from.name()))?,
            Provider::Ollama | Provider::Agent(..) => String::new(),
        };
        let base_url = match provider {
            Provider::OpenAi => Some(get(from, "BASE_URL").with_context(|| format!("{}_BASE_URL must be set for openai", from.name()))?),
            Provider::Ollama => Some(get(from, "BASE_URL").or_else(|| var("OLLAMA_HOST")).unwrap_or_else(|| "http://localhost:11434".into())),
            _ => None,
        };
        if matches!(provider, Provider::OpenAi) && model.is_none() {
            bail!("{}_MODEL must be set for openai", from.name());
        }
        let model = model.unwrap_or_else(|| default_model.into());
        return Ok(vec![AiConfig { provider, model, base_url, api_key, thinking, tier, auto: false }]);
    }
    let mut found: Vec<AiConfig> = AGENTS.iter()
        .filter_map(|a| Some(AiConfig {
            provider: Provider::Agent(a, find_bin(a.bin)?),
            model: a.models[tier as usize].into(),
            base_url: None,
            api_key: String::new(),
            thinking: thinking.clone(),
            tier,
            auto: true,
        }))
        .collect();
    if let Some(key) = var("GEMINI_API_KEY") {
        found.push(AiConfig { auto: true, thinking, ..AiConfig::gemini(key, tier) });
    }
    if found.is_empty() {
        bail!("no AI configured: set SMART_PROVIDER (and MID_/FAST_), set GEMINI_API_KEY, or install an agent CLI ({})",
            AGENTS.iter().map(|a| a.bin).collect::<Vec<_>>().join(", "));
    }
    Ok(found)
}

fn is_dead(config: &AiConfig) -> bool {
    matches!(&config.provider, Provider::Agent(a, _) if DEAD_AGENTS.lock().unwrap().contains(&a.name))
}

/// Which AI `tier` would use, for logs.
pub fn describe(tier: Tier) -> String {
    match resolve(tier) {
        Ok(configs) => configs.iter().find(|c| !is_dead(c)).map_or("none left".into(), |c| c.label()),
        Err(e) => format!("none ({e:#})"),
    }
}

/// Ask `tier`'s model. Auto-detected agents fall through to the next one on failure (usage limit,
/// not logged in, …) and stay skipped for the rest of the run; an explicitly set provider just fails.
pub async fn ask(tier: Tier, parts: &[Part], json_only: bool) -> Result<String> {
    let mut last_err = None;
    for config in resolve(tier)? {
        if config.auto && is_dead(&config) {
            continue;
        }
        match config.send(parts, json_only).await {
            Ok(text) => return Ok(text),
            Err(e) if config.auto => {
                tracing::warn!("{} failed, trying the next agent: {e:#}", config.label());
                if let Provider::Agent(a, _) = &config.provider {
                    DEAD_AGENTS.lock().unwrap().push(a.name);
                }
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("every auto-detected agent already failed in this run")))
}

pub async fn generate_input(files: &[(&str, &str)], config: Option<&AiConfig>) -> Result<String> {
    let parts = [Part::Text(input_generation_prompt(files))];
    match config {
        Some(c) => c.send(&parts, false).await,
        None => ask(Tier::Fast, &parts, false).await,
    }
}

async fn gemini(config: &AiConfig, parts: &[Part], json_only: bool) -> Result<String> {
    let client = Client::new();
    let mut last_err = None;

    for model in config.model.split(',').map(str::trim) {
        tracing::debug!("Trying Gemini model: {}", model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, config.api_key
        );
        let mut body = json!({ "contents": [{ "parts": gemini_parts(parts) }], "generationConfig": {} });
        if json_only {
            // thinking tokens count towards the output limit, so leave room for a full answer
            body["generationConfig"] = json!({"responseMimeType": "application/json", "maxOutputTokens": 65536});
        }
        if let Some(t) = &config.thinking {
            body["generationConfig"]["thinkingConfig"] = gemini_thinking(model, t);
        }

        let resp = match client.post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(anyhow!("Request failed for {}: {}", model, e));
                continue;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            let err_lower = err_text.to_lowercase();
            if status == reqwest::StatusCode::UNAUTHORIZED || (status == reqwest::StatusCode::BAD_REQUEST && (err_lower.contains("api_key") || err_lower.contains("api key") || err_lower.contains("key invalid"))) {
                bail!("Invalid API Key");
            }
            last_err = Some(anyhow!("HTTP error for {}: {} - {}", model, status, err_text));
            continue;
        }

        let res: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                last_err = Some(anyhow!("JSON error for {}: {}", model, e));
                continue;
            }
        };

        // long answers can arrive split across several parts
        let text: Option<String> = res["candidates"][0]["content"]["parts"].as_array()
            .map(|ps| ps.iter().filter_map(|p| p["text"].as_str()).collect::<String>())
            .filter(|t| !t.is_empty());
        tracing::trace!("Response text from {}: {:?}", model, text);

        let finish_reason = res["candidates"][0]["finishReason"].as_str();
        if finish_reason == Some("MAX_TOKENS") {
            last_err = Some(anyhow!("{} hit its output token limit (reply cut off)", model));
            continue;
        }
        match text {
            Some(t) => return Ok(t),
            // empty content with STOP means the program needs no input
            None if finish_reason == Some("STOP") => return Ok(String::new()),
            None => {
                last_err = Some(anyhow!("Failed to extract text from {}. Raw: {}", model, serde_json::to_string_pretty(&res).unwrap_or_default()));
            }
        }
    }

    bail!("All specified Gemini models failed. Last error: {:?}", last_err)
}

/// Gemini 2.x takes a token budget, later models a level (no xhigh, minimal instead of off).
fn gemini_thinking(model: &str, thinking: &str) -> Value {
    if model.starts_with("gemini-2") {
        let budget = match thinking { "off" => 0, "low" => 1024, "medium" => 8192, _ => 24576 };
        json!({ "thinkingBudget": budget })
    } else {
        let level = match thinking { "off" => "minimal", "xhigh" => "high", t => t };
        json!({ "thinkingLevel": level })
    }
}

/// Ollama's OpenAI-compatible API; with no model set, the first installed one.
async fn ollama(config: &AiConfig, parts: &[Part]) -> Result<String> {
    let base = config.base_url.as_deref().unwrap_or_default().trim_end_matches('/');
    let model = if config.model.is_empty() {
        let tags: Value = Client::new().get(format!("{base}/api/tags")).send().await
            .with_context(|| format!("ollama not reachable at {base}"))?.json().await?;
        tags["models"][0]["name"].as_str().context("ollama has no models installed (ollama pull <model>)")?.to_string()
    } else {
        config.model.clone()
    };
    openai(&AiConfig { model, base_url: Some(format!("{base}/v1")), ..config.clone() }, parts).await
}

async fn openai(config: &AiConfig, parts: &[Part]) -> Result<String> {
    let url = format!("{}/chat/completions", config.base_url.as_deref().unwrap_or_default());
    let mut body = json!({
        "model": config.model,
        "messages": [{"role": "user", "content": openai_parts(parts)}]
    });
    if let Some(t) = &config.thinking {
        body["reasoning_effort"] = json!(if t == "off" { "none" } else { t });
    }

    let mut resp = post_retrying(&url, config, &body).await?;
    if resp.status() == reqwest::StatusCode::BAD_REQUEST && config.thinking.is_some() {
        tracing::warn!("{} rejected reasoning_effort, retrying without it", config.model);
        body.as_object_mut().unwrap().remove("reasoning_effort");
        resp = post_retrying(&url, config, &body).await?;
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        let err_lower = err_text.to_lowercase();
        if status == reqwest::StatusCode::UNAUTHORIZED || err_lower.contains("invalid") || err_lower.contains("api_key") {
            bail!("Invalid API Key");
        }
        bail!("HTTP error: {} - {}", status, err_text);
    }

    let res: Value = resp.json().await?;
    let model = res["model"].as_str().unwrap_or(&config.model).to_string();
    let mut answered = ANSWERED_BY.lock().unwrap();
    match answered.iter_mut().find(|(m, _)| *m == model) {
        Some((_, n)) => *n += 1,
        None => answered.push((model, 1)),
    }
    drop(answered);
    res["choices"][0]["message"]["content"].as_str()
        .map(str::to_string)
        .context("Failed to extract text from OpenAI compatible response")
}

/// The models that answered openai requests, most used first.
pub fn answered_by() -> Vec<(String, usize)> {
    let mut answered = ANSWERED_BY.lock().unwrap().clone();
    answered.sort_by(|a, b| b.1.cmp(&a.1));
    answered
}

/// Gateways in front of free models time out or fail over now and then, so 5xx and 429 are retried
/// right away: a routing gateway picks another upstream on the next request anyway.
async fn post_retrying(url: &str, config: &AiConfig, body: &Value) -> Result<reqwest::Response> {
    for attempt in 0.. {
        let resp = Client::new().post(url).bearer_auth(&config.api_key).json(body).send().await;
        let transient = resp.as_ref().map_or(true, |r| r.status().is_server_error() || r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS);
        if !transient || attempt >= max_retries() {
            return Ok(resp?);
        }
        tracing::warn!("{} answered {}, retry {}/{}", config.model, resp.as_ref().map_or_else(|e| e.to_string(), |r| r.status().to_string()), attempt + 1, max_retries());
    }
    unreachable!()
}

/// `--retries` / `SAVECODEX_RETRIES`: how often a failed AI call (gateway error, rate limit, unusable reply) is retried.
pub fn max_retries() -> usize {
    var("SAVECODEX_RETRIES").and_then(|r| r.parse().ok()).unwrap_or(DEFAULT_RETRIES)
}

/// Agent burst limits clear with time: 5s, 10s, 20s, then 40s between retries.
fn backoff(attempt: usize) -> Duration {
    Duration::from_secs(5 << attempt.min(3))
}

/// Run an agent CLI with starts spaced out, and at most `SAVECODEX_AGENT_JOBS` per agent at once if set
/// (otherwise only `-j` limits it). Burst errors ("temporarily limiting", overloaded) are retried.
async fn run_agent(agent: &'static Agent, bin: &Path, model: &str, effort: Option<&str>, parts: &[Part]) -> Result<String> {
    let slots = AGENT_SLOTS.get_or_init(|| {
        let jobs = var("SAVECODEX_AGENT_JOBS").and_then(|j| j.parse().ok()).unwrap_or(Semaphore::MAX_PERMITS).max(1);
        AGENTS.iter().map(|_| Arc::new(Semaphore::new(jobs))).collect()
    });
    let slot = &slots[AGENTS.iter().position(|a| a.name == agent.name).unwrap_or(0)];
    let _permit = slot.acquire().await.expect("agent semaphore closed");

    for attempt in 0.. {
        {
            let mut last = LAST_AGENT_START.get_or_init(|| tokio::sync::Mutex::new(Instant::now() - AGENT_START_GAP)).lock().await;
            tokio::time::sleep(AGENT_START_GAP.saturating_sub(last.elapsed())).await;
            *last = Instant::now();
        }
        match run_agent_once(agent, bin, model, effort, parts).await {
            Err(e) if is_burst_limit(&format!("{e:#}")) && attempt < max_retries() => {
                tracing::warn!("{} is limiting requests, retry {}/{}", agent.name, attempt + 1, max_retries());
                tokio::time::sleep(backoff(attempt)).await;
            }
            result => return result,
        }
    }
    unreachable!()
}

fn is_burst_limit(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["temporarily limiting", "overloaded", "529"].iter().any(|p| lower.contains(p))
}

/// Run an agent CLI once in an empty temp dir. Images are saved there and referenced by path.
async fn run_agent_once(agent: &Agent, bin: &Path, model: &str, effort: Option<&str>, parts: &[Part]) -> Result<String> {
    let dir = tempfile::tempdir()?;
    let mut prompt = String::new();
    let mut images = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        match part {
            Part::Text(t) => prompt.push_str(t),
            Part::Image(mime, bytes) => {
                let path = dir.path().join(format!("image{i}.{}", mime.rsplit('/').next().unwrap_or("png")));
                std::fs::write(&path, bytes)?;
                let at = if agent.name == "gemini-cli" { "@" } else { "" };
                prompt.push_str(&format!("[image file: {at}{}]", path.display()));
                images.push(path);
            }
        }
        prompt.push('\n');
    }

    let last_message = dir.path().join("last_message.txt");
    let mut args: Vec<String> = Vec::new();
    let mut arg = |a: &str| args.push(a.to_string());
    let via_stdin = matches!(agent.name, "claude" | "codex");
    match agent.name {
        "claude" => {
            // no user MCP servers, hooks or plugins: they slow startup and leak into the prompt
            for a in ["-p", "--no-session-persistence", "--strict-mcp-config", "--setting-sources", "", "--tools", "Read", "--allowed-tools", "Read"] { arg(a) }
            if !model.is_empty() { arg("--model"); arg(model) }
            // claude has no "off"; low is its least
            if let Some(e) = effort { arg("--effort"); arg(if e == "off" { "low" } else { e }) }
        }
        "codex" => {
            // user config can require MCP servers that fail to start; auth lives outside it
            for a in ["exec", "--skip-git-repo-check", "--ephemeral", "--ignore-user-config", "-s", "read-only", "--color", "never", "-o"] { arg(a) }
            arg(&last_message.to_string_lossy());
            if !model.is_empty() { arg("-m"); arg(model) }
            if let Some(e) = effort { arg("-c"); arg(&format!("model_reasoning_effort=\"{}\"", if e == "off" { "minimal" } else { e })) }
            for img in &images { arg(&format!("--image={}", img.display())) }
        }
        "gemini-cli" | "qwen" => {
            if !model.is_empty() { arg("-m"); arg(model) }
            arg("-p"); arg(&prompt);
        }
        "cursor" => {
            arg("-p");
            if !model.is_empty() { arg("-m"); arg(model) }
            arg(&prompt);
        }
        "opencode" => {
            arg("run");
            if !model.is_empty() { arg("-m"); arg(model) }
            arg(&prompt);
        }
        "copilot" => {
            // shortcut: copilot's print mode needs this flag; it only runs in the empty temp dir
            arg("--allow-all-tools");
            if !model.is_empty() { arg("--model"); arg(model) }
            arg("-p"); arg(&prompt);
        }
        "antigravity" => {
            if !model.is_empty() { arg("--model"); arg(model) }
            if let Some(e) = effort { arg("--effort"); arg(if e == "off" { "low" } else { e }) }
            // -p takes the next arg as its prompt, so it goes last
            arg("-p"); arg(&prompt);
        }
        _ => { arg("-x"); arg(&prompt); }
    }

    tracing::debug!("Running agent {} ({} chars of prompt)", agent.name, prompt.len());
    let mut child = tokio::process::Command::new(bin)
        .args(&args)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("can't start {}", bin.display()))?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let input = if via_stdin { prompt } else { String::new() };
    // written from a task so a full stdout pipe can't deadlock us
    tokio::spawn(async move { let _ = stdin.write_all(input.as_bytes()).await; });

    let out = tokio::time::timeout(AGENT_TIMEOUT, child.wait_with_output()).await
        .map_err(|_| anyhow!("{} timed out after {}s", agent.name, AGENT_TIMEOUT.as_secs()))??;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = if agent.name == "codex" { std::fs::read_to_string(&last_message).unwrap_or_default() } else { stdout.clone() };

    if !out.status.success() {
        let detail = if stderr.trim().is_empty() { &stdout } else { &*stderr };
        bail!("{} exited with {}: {}", agent.name, out.status, tail(detail));
    }
    // some CLIs print a usage-limit or login notice and still exit 0
    if text.len() < 500 && looks_unusable(&format!("{text}\n{stderr}")) {
        bail!("{}: {}", agent.name, tail(&format!("{text}\n{stderr}")));
    }
    Ok(text)
}

fn looks_unusable(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["usage limit", "rate limit", "quota", "not logged in", "please log in", "login required", "/login", "credit balance", "unauthorized"]
        .iter().any(|p| lower.contains(p))
}

fn tail(text: &str) -> String {
    let t = text.trim();
    let start = t.char_indices().rev().nth(400).map_or(0, |(i, _)| i);
    t[start..].to_string()
}

#[cfg(test)]
#[path = "tests/ai.rs"]
mod tests;
