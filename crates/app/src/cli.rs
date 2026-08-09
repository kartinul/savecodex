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
        /// Folder containing source files.
        folder: PathBuf,

        /// Output file (without extension — both .docx and .pdf are written).
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Read questions from a PDF/DOCX, write code via AI, run it, export to DOCX + PDF.
    Solve {
        /// Assignment file (.pdf or .docx).
        input: PathBuf,

        /// Output file (without extension — both .docx and .pdf are written).
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Generate a terminal window screenshot (for dev/testing purposes).
    Term {
        /// Input file (reads stdin if omitted).
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Output PNG path.
        #[arg(short, long, default_value = "term.png")]
        output: PathBuf,

        /// Window title bar text.
        #[arg(short, long, default_value = "bash")]
        title: String,

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
        #[arg(long, default_value = "local")]
        username: String,

        /// Realistic prompt hostname.
        #[arg(long, default_value = "host")]
        hostname: String,

        /// Realistic prompt current working directory.
        #[arg(long, default_value = "~")]
        cwd: String,
    },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { host, port } => {
            server::start(&host, port).await?;
        }
        Commands::Pack { folder, output } => {
            println!("[pack] folder={} output={}", folder.display(), output.display());
            todo!("pack implementation")
        }
        Commands::Solve { input, output } => {
            println!("[solve] input={} output={}", input.display(), output.display());
            todo!("solve implementation")
        }
        Commands::Term {
            input,
            output,
            title,
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
            let mut raw = String::new();
            if let Some(path) = input {
                raw = std::fs::read_to_string(&path)?;
            } else {
                use std::io::Read;
                std::io::stdin().read_to_string(&mut raw)?;
            }

            let opts = crate::term_gen::TermGenOptions {
                title,
                font_size,
                theme,
                prompt,
                no_prompt_highlight,
                padding,
                style,
                username,
                hostname,
                cwd,
            };

            let img = crate::term_gen::generate_terminal_image(&raw, &opts)?;
            img.save(&output)?;
            println!("wrote {} ({}x{})", output.display(), img.width(), img.height());
        }
    }

    Ok(())
}
