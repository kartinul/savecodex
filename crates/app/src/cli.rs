//! CLI — `pack`, `solve`, `run`, `term` and `serve`.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::server;

/// savecodex — run code and turn it into documents.
#[derive(Parser)]
#[command(name = "savecodex", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// How the terminal screenshots look.
#[derive(Args)]
struct ShotArgs {
    /// Color theme (dark or light).
    #[arg(long, default_value = "dark")]
    theme: String,

    /// Prompt string to highlight.
    #[arg(long, default_value = "$ ")]
    prompt: String,

    /// Disable prompt highlighting.
    #[arg(long)]
    no_prompt_highlight: bool,

    /// Window style (windows, macos, ubuntu).
    #[arg(long, value_enum, default_value_t = crate::term_gen::WindowStyle::Windows, env = "SAVECODEX_STYLE")]
    style: crate::term_gen::WindowStyle,

    /// Realistic imperfections: 0 (off) to 3 (very messy).
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
    messy: u8,

    /// Seed for --messy (default: derived from the output text).
    #[arg(long)]
    seed: Option<u64>,

    /// Print the PowerShell startup banner (windows style).
    #[arg(long)]
    banner: bool,

    /// Minimum window width in columns (real terminals open at 80).
    #[arg(long, default_value_t = 80)]
    min_cols: u16,

    /// Realistic prompt username.
    #[arg(long)]
    username: Option<String>,

    /// Realistic prompt hostname.
    #[arg(long)]
    hostname: Option<String>,

    /// Realistic prompt current working directory.
    #[arg(long)]
    cwd: Option<String>,
}

impl ShotArgs {
    fn options(self) -> crate::term_gen::TermGenOptions {
        let username = self.username.unwrap_or_else(|| {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "local".to_string())
        });
        let hostname = self.hostname.unwrap_or_else(|| {
            hostname::get()
                .unwrap_or_else(|_| std::ffi::OsString::from("host"))
                .to_string_lossy()
                .into_owned()
        });
        let cwd = self.cwd.unwrap_or_else(|| {
            let Ok(path) = std::env::current_dir() else { return "~".to_string() };
            let path = path.to_string_lossy().into_owned();
            match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
                Ok(home) if path.starts_with(&home) => path.replacen(&home, "~", 1),
                _ => path,
            }
        });
        crate::term_gen::TermGenOptions {
            font_size: 18.0,
            theme: self.theme,
            prompt: self.prompt,
            no_prompt_highlight: self.no_prompt_highlight,
            padding: self.style.padding(),
            messy: self.messy,
            seed: self.seed,
            banner: self.banner,
            min_cols: self.min_cols,
            no_title_bar: false,
            style: self.style,
            username,
            hostname,
            cwd,
        }
    }
}

/// The generated DOCX and how each file is run.
#[derive(Args)]
struct DocArgs {
    /// Output file (without extension).
    #[arg(short, long, env = "SAVECODEX_OUTPUT")]
    output: Option<PathBuf>,

    /// Main heading for the document. Defaults to the folder or input file name.
    #[arg(long, env = "SAVECODEX_DOC_TITLE")]
    doc_title: Option<String>,

    /// Description text to appear below the main heading
    #[arg(long, env = "SAVECODEX_DOC_TEXT")]
    doc_text: Option<String>,

    /// Add a page break after each file's output.
    #[arg(long)]
    page_break: bool,

    /// How the document looks: a preset (current, word, word calibri, word 2010, academic, modern,
    /// lab record, compact, typewriter, elegant) or a theme .json file. Fields a file leaves out keep the current look.
    #[arg(long, default_value = "current", env = "SAVECODEX_DOC_THEME")]
    doc_theme: String,

    /// How sub-parts are numbered: 1(b)(i), 1(2)(1), 1.b.i or 1.2.1. Defaults to the theme's.
    #[arg(long, env = "SAVECODEX_NUMBERING")]
    numbering: Option<String>,

    /// Head sub-parts with their whole label (1(b)(i)) instead of only their own part ((i)).
    #[arg(long, env = "SAVECODEX_FULL_LABELS")]
    full_labels: bool,

    /// When to ask the AI for stdin: auto (only files that read stdin), always, or never.
    #[arg(long, value_enum, default_value_t, env = "SAVECODEX_AI_INPUT")]
    ai_input: crate::pack::AiInput,

    /// Don't trim long output to 150 lines / 200 chars per line (hard cap of 1000 lines / 300 chars still applies).
    #[arg(long, env = "SAVECODEX_NO_TRIM")]
    no_trim: bool,

    /// Max files (pack) or question groups (solve) to process in parallel.
    #[arg(short, long, default_value_t = 15, env = "SAVECODEX_JOBS")]
    jobs: usize,

    #[command(flatten)]
    linux: LinuxArgs,
}

impl DocArgs {
    fn theme(&self) -> anyhow::Result<crate::doc_theme::DocTheme> {
        let mut theme = crate::doc_theme::DocTheme::load(&self.doc_theme)?;
        if let Some(n) = &self.numbering {
            anyhow::ensure!(["1(b)(i)", "1(2)(1)", "1.b.i", "1.2.1"].contains(&n.as_str()), "--numbering must be 1(b)(i), 1(2)(1), 1.b.i or 1.2.1");
            theme.numbering = n.clone();
        }
        theme.full_labels |= self.full_labels;
        Ok(theme)
    }
}

/// Ctrl-C or a kill, so a stopped solve can clean up after itself.
async fn interrupted() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

#[derive(Args)]
struct LinuxArgs {
    /// Run code in a throwaway Linux container (one per document): Apple container on macOS if
    /// installed, else Docker. Nothing is installed for you. The image builds on first use;
    /// screenshots default to --style ubuntu.
    #[arg(long, env = "SAVECODEX_LINUX")]
    linux: bool,
}

impl LinuxArgs {
    fn apply(&self, shot: &mut crate::term_gen::TermGenOptions) -> Result<()> {
        if !self.linux { return Ok(()) }
        crate::linux::enable()?;
        let style_set = std::env::args().any(|a| a == "--style" || a.starts_with("--style=")) || std::env::var_os("SAVECODEX_STYLE").is_some();
        if !style_set {
            shot.style = crate::term_gen::WindowStyle::Ubuntu;
            shot.padding = shot.style.padding();
        }
        Ok(())
    }
}

/// Which AI each tier uses. Unset tiers fall back upwards (FAST → MID → SMART); with nothing set,
/// the first agent CLI found on PATH (claude, codex, gemini, ...) is used, else `GEMINI_API_KEY`.
/// Providers: gemini, openai, ollama, claude, codex, gemini-cli, cursor, opencode, copilot, qwen, amp, antigravity.
#[derive(Args)]
struct AiArgs {
    #[arg(short = 'p', long, visible_alias = "provider", env = "SMART_PROVIDER", help = crate::ai::provider_help())]
    smart_provider: Option<String>,

    /// Model for SMART (comma-separated Gemini models fall back in order).
    #[arg(long, env = "SMART_MODEL")]
    smart_model: Option<String>,

    /// Thinking for SMART: off, low, medium, high, xhigh. Unset keeps the provider's default.
    #[arg(long, env = "SMART_THINKING")]
    smart_thinking: Option<String>,

    /// Provider for MID: solving each question group in `solve`.
    #[arg(long, env = "MID_PROVIDER")]
    mid_provider: Option<String>,

    /// Model for MID.
    #[arg(long, env = "MID_MODEL")]
    mid_model: Option<String>,

    /// Thinking for MID.
    #[arg(long, env = "MID_THINKING")]
    mid_thinking: Option<String>,

    /// Provider for FAST: generating stdin for programs.
    #[arg(long, env = "FAST_PROVIDER")]
    fast_provider: Option<String>,

    /// Model for FAST.
    #[arg(long, env = "FAST_MODEL")]
    fast_model: Option<String>,

    /// Thinking for FAST (agents default to low here).
    #[arg(long, env = "FAST_THINKING")]
    fast_thinking: Option<String>,

    /// Retries for a failed AI call: gateway errors, rate limits and replies that don't parse [default: 5].
    #[arg(long, env = "SAVECODEX_RETRIES")]
    retries: Option<String>,

    /// Use local Ollama for every tier (first installed model unless *_MODEL is set; OLLAMA_HOST for another host).
    #[arg(long, env = "SAVECODEX_LOCAL")]
    local: bool,
}

impl AiArgs {
    fn apply(mut self) {
        if self.local {
            let ollama = || Some("ollama".to_string());
            (self.smart_provider, self.mid_provider, self.fast_provider) = (ollama(), ollama(), ollama());
        }
        // `-p` on the command line sets every tier, over any other provider and env model
        let given = |flag: &str| std::env::args().any(|a| a == flag || a.starts_with(&format!("{flag}=")));
        if given("--provider") || given("--smart-provider") || std::env::args().any(|a| a.starts_with("-p")) {
            let chosen = self.smart_provider.clone();
            for (provider, model, tier) in [(&mut self.smart_provider, &mut self.smart_model, "smart"), (&mut self.mid_provider, &mut self.mid_model, "mid"), (&mut self.fast_provider, &mut self.fast_model, "fast")] {
                *provider = chosen.clone();
                if !given(&format!("--{tier}-model")) { *model = Some(String::new()) }
            }
        }
        let values = [
            ("SMART_PROVIDER", self.smart_provider), ("SMART_MODEL", self.smart_model), ("SMART_THINKING", self.smart_thinking),
            ("MID_PROVIDER", self.mid_provider), ("MID_MODEL", self.mid_model), ("MID_THINKING", self.mid_thinking),
            ("FAST_PROVIDER", self.fast_provider), ("FAST_MODEL", self.fast_model), ("FAST_THINKING", self.fast_thinking),
            ("SAVECODEX_RETRIES", self.retries),
        ];
        crate::ai::set_overrides(values.into_iter().filter_map(|(k, v)| Some((k.to_string(), v?))).collect());
    }
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Start the HTTP API server.
    Serve {
        /// Address to bind to.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port to listen on.
        #[arg(long, default_value_t = 7878)]
        port: u16,

        /// Enable debug logging.
        #[arg(long)]
        debug: bool,

        #[command(flatten)]
        ai: AiArgs,
    },

    /// Run code files in a folder and package them with their output into a DOCX.
    Pack {
        /// Files or folders to pack.
        #[arg(help = "Files or folders to pack.", required = true)]
        folders: Vec<PathBuf>,

        /// Recurse into subdirectories. Without this flag, only the given folder itself is packed.
        #[arg(short, long)]
        recursive: bool,

        /// Comma-separated list of file extensions to include (e.g., "java,py,rs")
        #[arg(long, value_delimiter = ',', env = "SAVECODEX_EXT")]
        ext: Vec<String>,

        #[command(flatten)]
        doc: DocArgs,

        #[command(flatten)]
        shot: ShotArgs,

        #[command(flatten)]
        ai: AiArgs,
    },

    /// Read questions from PDF/DOCX/images, solve them with AI, run every answer and write a DOCX.
    ///
    /// Each question becomes code + terminal screenshot, an ER/UML diagram, or a written answer.
    /// SQL and shell commands are typed into a real `mysql>` / `$` session. SMART extracts the
    /// questions, MID solves them. With several PDF/DOCX inputs each one is solved into its own
    /// DOCX (other inputs are shared by all; --output needs '{}'). Needs poppler (pdftotext,
    /// pdftoppm), plus mysql and graphviz (dot) for SQL and diagram questions. AI-written code and
    /// commands run on this machine.
    Solve {
        /// Input files. PDFs, DOCX and images are read; other files (data like records.txt)
        /// are also copied next to the answers so programs can open them.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,

        /// Print each question's text above its answer.
        #[arg(long)]
        questions: bool,

        /// Refuse PDFs longer than this many pages.
        #[arg(long, default_value_t = 60)]
        max_pages: usize,

        /// Have the SMART model grade every run and diagram (good or bad, how sure, what went wrong)
        /// and redo the answers it grades bad at least this sure. Off unless given; alone it means 0.7.
        #[arg(long, env = "SAVECODEX_CHECK", num_args = 0..=1, default_missing_value = "0.7", value_name = "CONFIDENCE")]
        check: Option<f32>,

        /// How professional the code looks: 1 is a careless student (meaningless names, broken
        /// indentation), 3 a student in a hurry (lazy names, ~80% tidy indentation), 5 clean code.
        #[arg(long, env = "SAVECODEX_LEVEL", default_value_t = 3, value_parser = clap::value_parser!(u8).range(1..=5))]
        level: u8,

        /// Your own instructions for the AI (e.g. "use Java", "comment every line"). Every prompt puts
        /// them first and they win over savecodex's own rules, except the reply format.
        #[arg(long, env = "SAVECODEX_INSTRUCTIONS")]
        instructions: Option<String>,

        #[command(flatten)]
        doc: DocArgs,

        #[command(flatten)]
        shot: ShotArgs,

        #[command(flatten)]
        ai: AiArgs,
    },

    /// Compile and run a source code file using pseudo-terminal.
    Run {
        /// Source file to execute.
        file: PathBuf,

        /// Optional stdin input to send to the program.
        #[arg(short, long)]
        input: Option<String>,

        /// Optional output file to write the text transcript to instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,

        #[command(flatten)]
        linux: LinuxArgs,
    },

    /// Generate a terminal window screenshot (for dev/testing purposes).
    Term {
        /// Source file to execute and render.
        #[arg(long)]
        runfile: Option<PathBuf>,

        /// Optional stdin input to send to the executing program (used with --runfile).
        #[arg(short, long)]
        input: Option<String>,

        /// Text file to render (if not using --runfile). Reads from stdin if omitted.
        #[arg(long)]
        text_file: Option<PathBuf>,

        /// Output PNG path.
        #[arg(short, long, default_value = "term.png")]
        output: PathBuf,

        /// Don't draw a prompt line above the text (or above the command with --runfile).
        #[arg(long, alias = "noprompt")]
        no_prompt: bool,

        #[command(flatten)]
        shot: ShotArgs,
    },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { host, port, debug: _, ai } => {
            ai.apply();
            server::start(&host, port).await?;
        }
        Commands::Pack { folders, recursive, ext, doc, shot, ai } => {
            ai.apply();
            let mut opts = shot.options();
            doc.linux.apply(&mut opts)?;
            if doc.ai_input != crate::pack::AiInput::Never {
                println!("🤖 Input AI: {}", crate::ai::describe(crate::ai::Tier::Fast));
            }

            if folders.len() > 1 {
                if let Some(out) = &doc.output {
                    if !out.to_string_lossy().contains("{}") {
                        anyhow::bail!("When packing multiple inputs, the --output flag must contain the '{{}}' placeholder to avoid overwriting files.");
                    }
                }
            }

            for folder in folders {
                let folder_name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("folder");
                
                let resolved_output = doc.output.as_ref().map(|o| {
                    PathBuf::from(o.to_string_lossy().replace("{}", folder_name))
                });
                
                let resolved_doc_title = doc.doc_title.as_ref().map(|t| t.replace("{}", folder_name));
                let resolved_doc_text = doc.doc_text.as_ref().map(|t| t.replace("{}", folder_name));

                println!("📦 Packing: {}", folder.display());
                println!("   🔍 Recursive: {}", if recursive { "yes" } else { "no (folder only)" });
                if let Some(ref out) = resolved_output {
                    println!("   📄 Output: {}", out.display());
                }
                println!("   🎨 Style: {:?}", opts.style);
                if let Some(ref title) = resolved_doc_title {
                    println!("   📝 Title: {}", title);
                }
                if let Some(ref text) = resolved_doc_text {
                    println!("   ℹ️  Text: {}", text.replace('\n', "\\n"));
                }

                crate::pack::run_pack(&folder, resolved_output.as_deref(), &ext, resolved_doc_title.as_deref(), resolved_doc_text.as_deref(), doc.page_break, &doc.theme()?, &opts, doc.jobs, recursive, doc.ai_input, !doc.no_trim, None).await?;
            }
        }
        Commands::Solve { inputs, questions, max_pages, check, level, instructions, doc, shot, ai } => {
            ai.apply();
            let custom_cwd = shot.cwd.is_some();
            let mut term = shot.options();
            doc.linux.apply(&mut term)?;
            // answers run in their own home dir, so the prompt shows ~ unless --cwd says otherwise
            if !custom_cwd { term.cwd = "~".into(); }

            let is_doc = |p: &PathBuf| p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["pdf", "docx"].contains(&e.to_lowercase().as_str()));
            let (docs, shared): (Vec<PathBuf>, Vec<PathBuf>) = inputs.into_iter().partition(is_doc);
            let runs: Vec<Vec<PathBuf>> = if docs.len() > 1 {
                docs.into_iter().map(|d| [vec![d], shared.clone()].concat()).collect()
            } else {
                vec![docs.into_iter().chain(shared).collect()]
            };
            if runs.len() > 1 && doc.output.as_ref().is_some_and(|o| !o.to_string_lossy().contains("{}")) {
                anyhow::bail!("When solving multiple documents, the --output flag must contain the '{{}}' placeholder to avoid overwriting files.");
            }

            let theme = doc.theme()?;
            let multi = runs.len() > 1;
            let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(doc.jobs.max(1)));
            let mut set = tokio::task::JoinSet::new();
            for (i, run) in runs.into_iter().enumerate() {
                let name = run[0].file_stem().and_then(|n| n.to_str()).unwrap_or("solved").to_string();
                println!("📚 Solving: {}", run[0].display());
                let opts = crate::solve::SolveOptions {
                    term: term.clone(),
                    output: doc.output.as_ref().map(|o| PathBuf::from(o.to_string_lossy().replace("{}", &name))),
                    doc_title: doc.doc_title.as_ref().map(|t| t.replace("{}", &name)),
                    doc_text: doc.doc_text.as_ref().map(|t| t.replace("{}", &name)),
                    page_break: doc.page_break,
                    theme: theme.clone(),
                    trim: !doc.no_trim,
                    jobs: doc.jobs,
                    questions,
                    max_pages,
                    check,
                    level,
                    sem: sem.clone(),
                    tag: if multi { name.clone() } else { String::new() },
                    events: None,
                };
                let rules = crate::ai::RunAi { config: None, instructions: instructions.clone() };
                set.spawn(crate::ai::scoped(rules, async move { (i, name, crate::solve::run_solve(&run, opts).await) }));
            }
            let mut results = tokio::select! {
                results = set.join_all() => results,
                _ = interrupted() => {
                    crate::solve::cleanup_after_interrupt().await;
                    anyhow::bail!("interrupted");
                }
            };
            results.sort_by_key(|(i, ..)| *i);
            crate::solve::print_stats();

            if !multi {
                return results.pop().map_or(Ok(()), |(.., r)| r.map(drop));
            }
            let width = results.iter().map(|(_, name, _)| name.len()).max().unwrap_or(0);
            println!("\nSummary:");
            for (_, name, result) in &results {
                match result {
                    Ok(summary) => println!("  {name:width$}  {summary}"),
                    Err(e) => println!("  {name:width$}  failed: {e:#}"),
                }
            }
            let failed = results.iter().filter(|(.., r)| r.is_err()).count();
            if failed > 0 {
                anyhow::bail!("{failed} of {} document(s) failed", results.len());
            }
        }
        Commands::Run { file, input, output, linux } => {
            if linux.linux { crate::linux::enable()?; }
            let dir = std::fs::canonicalize(&file)?.parent().map(PathBuf::from).unwrap_or_default();
            let _linux = crate::linux::Container::start(&dir).await?;
            let (_cmd_str, out) = crate::runner::run_file(&file, input.as_deref()).await?;
            if let Some(path) = output {
                std::fs::write(&path, out)?;
                println!("Wrote execution output to {}", path.display());
            } else {
                print!("{}", out);
            }
        }
        Commands::Term { runfile, input, text_file, output, no_prompt, shot } => {
            let opts = shot.options();
            let mut raw = String::new();
            
            if let Some(rf) = runfile {
                let (exec_cmd, out) = crate::runner::run_file(&rf, input.as_deref()).await?;
                raw = if no_prompt { out } else { format!("{}{}\n{}", opts.prompt, exec_cmd, out) };
            } else {
                if let Some(path) = text_file {
                    raw = std::fs::read_to_string(&path)?;
                } else {
                    use std::io::Read;
                    std::io::stdin().read_to_string(&mut raw)?;
                }
                if !no_prompt && !raw.starts_with(&opts.prompt) {
                    raw = format!("{}\n{}", opts.prompt, raw);
                }
            }

            let img = crate::term_gen::generate_terminal_image(&raw, &opts)?;
            img.save(&output)?;
            println!("wrote {} ({}x{})", output.display(), img.width(), img.height());
        }
    }

    Ok(())
}
