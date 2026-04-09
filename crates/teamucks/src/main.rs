use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "teamucks", about = "A modern terminal multiplexer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Attach to an existing session
    Attach {
        /// Session name
        session: Option<String>,
    },
    /// List sessions
    List,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    tracing::info!("teamucks starting");
    drop(cli);
    println!("teamucks: not yet implemented");
    Ok(())
}
