mod commands;

use clap::{CommandFactory, Parser, ValueEnum};
use clap_complete::{Shell, generate};
use std::io;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "release-kit")]
#[command(version, about = "Static site generator for album releases", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Parser)]
enum Command {
    /// Initialize new album directory
    Init {
        /// Path to create album directory
        path: PathBuf,
    },

    /// Validate album configuration
    Validate {
        /// Path to album directory
        path: PathBuf,
    },

    /// Preview site locally with hot reload
    Preview {
        /// Path to album directory
        path: PathBuf,

        /// Port to serve on
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },

    /// Build site without deploying
    Build {
        /// Path to album directory
        path: PathBuf,

        /// Output directory for generated site
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Deploy site to hosting platform
    Deploy {
        #[command(subcommand)]
        command: DeployCommand,
    },

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Parser)]
enum DeployCommand {
    /// Configure Cloudflare credentials and base domain
    ///
    /// Required API Token Permissions:
    ///   Account > Cloudflare Pages > Edit
    ///   Zone > DNS > Edit (for custom domains)
    ///   Zone > Zone > Read (for custom domains)
    ///
    /// Create token at: https://dash.cloudflare.com/profile/api-tokens
    Configure,

    /// Publish album to Cloudflare Pages
    Publish {
        /// Path to album directory
        path: PathBuf,

        /// Skip confirmation prompts
        #[arg(long)]
        force: bool,
    },

    /// Show deployment status and info
    Status {
        /// Path to album directory (optional - scans current dir)
        path: Option<PathBuf>,
    },

    /// Delete deployment from Cloudflare Pages
    Teardown {
        /// Path to album directory
        path: PathBuf,

        /// Skip confirmation prompt (dangerous!)
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum DeployTarget {
    Cloudflare,
    // Future: Netlify, Static
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { path } => commands::init::run(path).await,
        Command::Validate { path } => commands::validate::run(path).await,
        Command::Preview { path, port } => commands::preview::run(path, port).await,
        Command::Build { path, output } => commands::build::run(path, output).await,
        Command::Deploy { command } => match command {
            DeployCommand::Configure => {
                commands::deploy::configure().await
            }
            DeployCommand::Publish { path, force } => {
                commands::deploy::publish(path, force).await
            }
            DeployCommand::Status { path } => {
                commands::deploy::status(path).await
            }
            DeployCommand::Teardown { path, force } => {
                commands::deploy::teardown(path, force).await
            }
        },
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "release-kit", &mut io::stdout());
            Ok(())
        }
    }
}
