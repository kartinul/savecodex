//! CLI — `pack`, `solve`, `run`, `term` and `serve`.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::server;

/// savecodex — turn assignments into documents.
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

    /// When to ask the AI for stdin: auto (only files that read stdin), always, or never.
    #[arg(long, value_enum, default_value_t, env = "SAVECODEX_AI_INPUT")]
    ai_input: crate::pack::AiInput,

    /// Don't trim long output to 150 lines / 200 chars per line (hard cap of 1000 lines / 300 chars still applies).
    #[arg(long, env = "SAVECODEX_NO_TRIM")]
    no_trim: bool,

    /// Max files (pack) or question groups (solve) to process in parallel.
    #[arg(short, long, default_value_t = 15, env = "SAVECODEX_JOBS")]
    jobs: usize,
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

        /// Use the server's env AI config; users need no API key or admin password.
        #[arg(long, env = "SAVECODEX_NO_KEY")]
        no_key: bool,
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
    },

    /// Read an assignment (PDF/DOCX/images), solve it with AI, run every answer and write a DOCX.
    ///
    /// Each question becomes code + terminal screenshot, an ER/UML diagram, or a written answer.
    /// SQL and shell commands are typed into a real `mysql>` / `$` session. Uses the CODE_* model;
    /// pick a strong one. Needs poppler (pdftotext, pdftoppm), plus mysql and graphviz (dot) for
    /// SQL and diagram questions. AI-written code and commands run on this machine.
    Solve {
        /// Assignment files. PDFs, DOCX and images are read; other files (data like student.txt)
        /// are also copied next to the answers so programs can open them.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,

        /// Print each question's text above its answer.
        #[arg(long)]
        questions: bool,

        /// Refuse PDFs longer than this many pages.
        #[arg(long, default_value_t = 60)]
        max_pages: usize,

        #[command(flatten)]
        doc: DocArgs,

        #[command(flatten)]
        shot: ShotArgs,
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
        Commands::Serve { host, port, debug: _, no_key } => {
            server::start(&host, port, no_key).await?;
        }
        Commands::Pack { folders, recursive, ext, doc, shot } => {
            let opts = shot.options();

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

                crate::pack::run_pack(&folder, resolved_output.as_deref(), &ext, resolved_doc_title.as_deref(), resolved_doc_text.as_deref(), doc.page_break, &opts, None, doc.jobs, recursive, doc.ai_input, !doc.no_trim).await?;
            }
        }
        Commands::Solve { inputs, questions, max_pages, doc, shot } => {
            let name = inputs[0].file_stem().and_then(|n| n.to_str()).unwrap_or("solved").to_string();
            let opts = crate::solve::SolveOptions {
                term: shot.options(),
                output: doc.output.map(|o| PathBuf::from(o.to_string_lossy().replace("{}", &name))),
                doc_title: doc.doc_title.map(|t| t.replace("{}", &name)),
                doc_text: doc.doc_text.map(|t| t.replace("{}", &name)),
                page_break: doc.page_break,
                ai_input: doc.ai_input,
                trim: !doc.no_trim,
                jobs: doc.jobs,
                questions,
                max_pages,
            };
            crate::solve::run_solve(&inputs, opts).await?;
        }
        Commands::Run { file, input, output } => {
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
