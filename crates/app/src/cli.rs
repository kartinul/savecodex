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
    }

    Ok(())
}
