//! CLI — `pack` and `solve`.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::server;

/// SaveCodex — turn assignments into documents.
#[derive(Parser)]
#[command(name = "savecodex", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Format enum removed since we only support Docx now

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
    },

    /// Run code files in a folder and package them with output into DOCX + PDF.
    Pack {
        /// Files or folders to pack.
        #[arg(help = "Files or folders to pack.", required = true)]
        folders: Vec<PathBuf>,

        /// Output file (without extension). Defaults to <folder_name>_pack.
        #[arg(short, long, env = "SAVECODEX_OUTPUT")]
        output: Option<PathBuf>,

        /// Comma-separated list of file extensions to include (e.g., "java,py,rs")
        #[arg(long, value_delimiter = ',', env = "SAVECODEX_EXT")]
        ext: Vec<String>,

        /// Main heading for the document. Defaults to folder name.
        #[arg(long, env = "SAVECODEX_DOC_TITLE")]
        doc_title: Option<String>,

        /// Description text to appear below the main heading
        #[arg(long, env = "SAVECODEX_DOC_TEXT")]
        doc_text: Option<String>,

        // Output format is always DOCX now



        /// Font size in pixels.
        #[arg(long, default_value_t = 18.0)]
        font_size: f32,

        /// Color theme (dark or light).
        #[arg(long, default_value = "dark")]
        theme: String,

        /// Prompt string to highlight.
        #[arg(long, default_value = "$ ")]
        prompt: String,

        /// Disable prompt highlighting.
        #[arg(long)]
        no_prompt_highlight: bool,

        /// Padding around text.
        #[arg(long, default_value_t = 28)]
        padding: i32,

        /// Window style (windows, macos, linux).
        #[arg(long, value_enum, default_value_t = crate::term_gen::WindowStyle::Windows, env = "SAVECODEX_STYLE")]
        style: crate::term_gen::WindowStyle,

        /// Realistic prompt username.
        #[arg(long)]
        username: Option<String>,

        /// Realistic prompt hostname.
        #[arg(long)]
        hostname: Option<String>,

        /// Realistic prompt current working directory.
        #[arg(long)]
        cwd: Option<String>,

        /// Add a page break after each file's output.
        #[arg(long)]
        page_break: bool,
    },

    /// Read questions from a PDF/DOCX, write code via AI, run it, export to DOCX + PDF.
    Solve {
        /// Assignment file (.pdf or .docx).
        input: PathBuf,

        /// Output file (without extension — both .docx and .pdf are written).
        #[arg(short, long)]
        output: PathBuf,
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
        #[arg(long)]
        input: Option<String>,

        /// Text file to render (if not using --runfile). Reads from stdin if omitted.
        #[arg(long)]
        text_file: Option<PathBuf>,

        /// Output PNG path.
        #[arg(short, long, default_value = "term.png")]
        output: PathBuf,



        /// Font size in pixels.
        #[arg(long, default_value_t = 18.0)]
        font_size: f32,

        /// Color theme (dark or light).
        #[arg(long, default_value = "dark")]
        theme: String,

        /// Prompt string to highlight.
        #[arg(long, default_value = "$ ")]
        prompt: String,

        /// Disable prompt highlighting.
        #[arg(long)]
        no_prompt_highlight: bool,

        /// Padding around text.
        #[arg(long, default_value_t = 28)]
        padding: i32,

        /// Window style (windows, macos, linux).
        #[arg(long, value_enum, default_value_t = crate::term_gen::WindowStyle::Windows)]
        style: crate::term_gen::WindowStyle,

        /// Realistic prompt username.
        #[arg(long)]
        username: Option<String>,

        /// Realistic prompt hostname.
        #[arg(long)]
        hostname: Option<String>,

        /// Realistic prompt current working directory.
        #[arg(long)]
        cwd: Option<String>,
    },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { host, port } => {
            server::start(&host, port).await?;
        }
        Commands::Pack {
            folders,
            output,
            ext,
            doc_title,
            doc_text,
            font_size,
            theme,
            prompt,
            no_prompt_highlight,
            padding,
            style,
            username,
            hostname,
            cwd,
            page_break,
        } => {
            let resolved_username = username.unwrap_or_else(|| {
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "local".to_string())
            });

            let resolved_hostname = hostname.unwrap_or_else(|| {
                hostname::get()
                    .unwrap_or_else(|_| std::ffi::OsString::from("host"))
                    .to_string_lossy()
                    .into_owned()
            });

            let resolved_cwd = cwd.unwrap_or_else(|| {
                if let Ok(path) = std::env::current_dir() {
                    let mut path_str = path.to_string_lossy().into_owned();
                    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
                        if path_str.starts_with(&home) {
                            path_str = path_str.replacen(&home, "~", 1);
                        }
                    }
                    path_str
                } else {
                    "~".to_string()
                }
            });

            let opts = crate::term_gen::TermGenOptions {
                font_size,
                theme,
                prompt,
                no_prompt_highlight,
                padding,
                style,
                username: resolved_username,
                hostname: resolved_hostname,
                cwd: resolved_cwd,
            };

            if folders.len() > 1 {
                if let Some(out) = &output {
                    if !out.to_string_lossy().contains("{}") {
                        anyhow::bail!("When packing multiple inputs, the --output flag must contain the '{{}}' placeholder to avoid overwriting files.");
                    }
                }
            }

            for folder in folders {
                let folder_name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("folder");
                
                let resolved_output = output.as_ref().map(|o| {
                    PathBuf::from(o.to_string_lossy().replace("{}", folder_name))
                });
                
                let resolved_doc_title = doc_title.as_ref().map(|t| t.replace("{}", folder_name));
                let resolved_doc_text = doc_text.as_ref().map(|t| t.replace("{}", folder_name));

                println!("📦 Packing: {}", folder.display());
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

                crate::pack::run_pack(&folder, resolved_output.as_deref(), &ext, resolved_doc_title.as_deref(), resolved_doc_text.as_deref(), page_break, &opts).await?;
            }
        }
        Commands::Solve { input, output } => {
            println!("[solve] input={} output={}", input.display(), output.display());
            todo!("solve implementation")
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
        Commands::Term {
            runfile,
            input,
            text_file,
            output,
            font_size,
            theme,
            prompt,
            no_prompt_highlight,
            padding,
            style,
            username,
            hostname,
            cwd,
        } => {
            let resolved_username = username.unwrap_or_else(|| {
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "local".to_string())
            });

            let resolved_hostname = hostname.unwrap_or_else(|| {
                hostname::get()
                    .unwrap_or_else(|_| std::ffi::OsString::from("host"))
                    .to_string_lossy()
                    .into_owned()
            });

            let resolved_cwd = cwd.unwrap_or_else(|| {
                if let Ok(path) = std::env::current_dir() {
                    let mut path_str = path.to_string_lossy().into_owned();
                    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
                        if path_str.starts_with(&home) {
                            path_str = path_str.replacen(&home, "~", 1);
                        }
                    }
                    path_str
                } else {
                    "~".to_string()
                }
            });

            let mut raw = String::new();
            
            if let Some(rf) = runfile {
                let (exec_cmd, out) = crate::runner::run_file(&rf, input.as_deref()).await?;
                // Construct fake prompt
                let fake_prompt = format!("{}{}\n", prompt, exec_cmd);
                raw = format!("{}{}", fake_prompt, out);
            } else if let Some(path) = text_file {
                raw = std::fs::read_to_string(&path)?;
            } else {
                use std::io::Read;
                std::io::stdin().read_to_string(&mut raw)?;
            }

            let opts = crate::term_gen::TermGenOptions {
                font_size,
                theme,
                prompt,
                no_prompt_highlight,
                padding,
                style,
                username: resolved_username,
                hostname: resolved_hostname,
                cwd: resolved_cwd,
            };

            let img = crate::term_gen::generate_terminal_image(&raw, &opts)?;
            img.save(&output)?;
            println!("wrote {} ({}x{})", output.display(), img.width(), img.height());
        }
    }

    Ok(())
}
