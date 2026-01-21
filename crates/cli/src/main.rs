use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "snapshort-cli")]
#[command(about = "Snapshort Video Editor CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new project
    New { name: String },
    /// Analyze media file
    Analyze { file: std::path::PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New { name } => {
            println!("Creating project: {}", name);
        }
        Commands::Analyze { file } => {
            println!("Analyzing: {}", file.display());
        }
    }

    Ok(())
}
