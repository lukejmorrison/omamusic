use clap::{Parser, Subcommand};
use omamusic::paths::AppPaths;
use omamusic::protocol::{BACKEND_VERSION, PROTOCOL_VERSION};
use omamusic::server;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "omamusic",
    version = BACKEND_VERSION,
    about = "Oma Music — YouTube Music playback for Omarchy (mpv + yt-dlp)",
    after_help = "Playback commands (talk to the daemon): status, play [query], pause, next, prev, stop, toggle,\nvolume N, seek M:SS, shuffle on|off, repeat off|all|one, search QUERY, queue, browse VIEW,\nlike, unlike, play-id VIDEO_ID, raw COMMAND [JSON], open, mini.\nAgents: pass --json and treat \"ok\":true as success."
)]
struct Args {
    /// Print the backend JSON reply (default when stdout is not a TTY)
    #[arg(long)]
    json: bool,
    /// Pretty text even when stdout is not a TTY
    #[arg(long)]
    human: bool,
    /// Fail if the socket is missing instead of starting the service
    #[arg(long)]
    no_start: bool,
    /// Seconds to wait for a reply
    #[arg(long, default_value_t = 15)]
    timeout: u64,
    /// Override the Unix socket path
    #[arg(long)]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the playback daemon (NDJSON Unix socket)
    Serve {
        #[arg(long)]
        auth: Option<PathBuf>,
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        self_test: bool,
    },
    /// Protocol self-check
    SelfTest,
    #[command(external_subcommand)]
    Other(Vec<String>),
}

fn main() -> ExitCode {
    let args = Args::parse();
    match args.command {
        Some(Command::Serve {
            auth,
            socket,
            self_test,
        }) => {
            if self_test {
                return match server::self_test() {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(err) => {
                        eprintln!("omamusic: {err}");
                        ExitCode::from(1)
                    }
                };
            }
            let paths = AppPaths::from_env();
            let backend = server::Backend::new(paths.clone(), auth);
            let path = socket.unwrap_or_else(|| paths.socket_path());
            match server::serve(backend, path) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("omamusic: {err}");
                    ExitCode::from(1)
                }
            }
        }
        Some(Command::SelfTest) => match server::self_test() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("omamusic: {err}");
                ExitCode::from(1)
            }
        },
        Some(Command::Other(rest)) => run_cli(
            args.json,
            args.human,
            args.no_start,
            args.timeout,
            args.socket,
            rest,
        ),
        None => {
            eprintln!(
                "omamusic {BACKEND_VERSION} (protocol {PROTOCOL_VERSION})\nTry `omamusic --help` or `omamusic serve`."
            );
            ExitCode::from(2)
        }
    }
}

fn run_cli(
    json: bool,
    human: bool,
    no_start: bool,
    timeout: u64,
    socket: Option<PathBuf>,
    rest: Vec<String>,
) -> ExitCode {
    let paths = AppPaths::from_env();
    let code = omamusic::cli::run_command(
        &paths,
        socket,
        !no_start,
        Duration::from_secs(timeout),
        json,
        human,
        &rest,
    );
    ExitCode::from(code as u8)
}
