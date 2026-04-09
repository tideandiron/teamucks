#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use clap::Parser;
use teamucks_core::server::{default_socket_path, Server};

#[derive(Parser, Debug)]
#[command(name = "teamucks", about = "A modern terminal multiplexer")]
struct Cli {
    /// Server name (selects the socket to connect to or create)
    #[arg(long, default_value = "default")]
    server: String,

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
    /// Start the server in the foreground (Phase 1)
    StartServer,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    tracing::info!(server = %cli.server, "teamucks starting");

    match cli.command {
        None | Some(Command::StartServer) => {
            start_server(&cli.server);
        }
        Some(Command::Attach { session }) => {
            println!("teamucks: attach not yet implemented (session: {session:?})");
        }
        Some(Command::List) => println!("teamucks: list not yet implemented"),
    }
}

/// Start the server in the foreground using a new tokio runtime.
///
/// In Phase 1 this blocks until the process is killed. Full daemonize
/// (fork + background) will be added in a later feature.
fn start_server(server_name: &str) {
    let socket_path = default_socket_path(server_name);
    tracing::info!(socket = %socket_path.display(), "starting server");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime must be constructable");
    rt.block_on(async move {
        let mut server = match Server::bind(&socket_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("teamucks: failed to start server: {e}");
                std::process::exit(1);
            }
        };
        tracing::info!(socket = %server.socket_path().display(), "server ready");
        if let Err(e) = server.run().await {
            tracing::error!(error = %e, "server error");
            server.shutdown();
        }
    });
}
