#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

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

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    tracing::info!("teamucks starting");
    match cli.command {
        None => println!("teamucks: not yet implemented"),
        Some(Command::Attach { session }) => {
            println!("teamucks: attach not yet implemented (session: {session:?})");
        }
        Some(Command::List) => println!("teamucks: list not yet implemented"),
    }
}
