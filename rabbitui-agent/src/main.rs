//! The `rabbit` binary: a terminal chat/agent client.
//!
//! A thin entry point over [`rabbitui_agent`]: parse arguments, pick a backend,
//! open (or resume) a session, and run. Slice 1 has no network backend, so the
//! default is the built-in [`DemoBackend`]; slice 2 makes the real Anthropic
//! backend the default.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use rabbitui_agent::app::{self, Agent};
use rabbitui_agent::backend::Backend;
use rabbitui_agent::backend::anthropic::AnthropicBackend;
use rabbitui_agent::backend::replay::ReplayBackend;
use rabbitui_agent::demo::DemoBackend;
use rabbitui_agent::session::Session;

const USAGE: &str = "\
rabbit — a terminal chat/agent client (rabbitui flagship)

USAGE:
    rabbit [OPTIONS]

By default rabbit talks to the Anthropic API. Export ANTHROPIC_API_KEY or
ANTHROPIC_AUTH_TOKEN (they must be exported, not just set), or run `ant auth login`
then `set -a; eval \"$(ant auth print-credentials --env)\"; set +a`. Without
credentials it falls back to an offline demo. ANTHROPIC_BASE_URL is honored.

OPTIONS:
    --model <ID>      Model id to request (default: claude-opus-4-8)
    --theme <FILE>    Load a TOML theme file (see themes/example.toml)
    --continue        Resume the most recent session
    --resume <FILE>   Resume a specific session file
    --replay <FILE>   Play a JSONL replay fixture instead of calling the API
    --demo            Use the built-in offline demo backend
    -h, --help        Print this help

A theme file may also be set via the RABBIT_THEME environment variable; the
--theme flag wins when both are present.
";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rabbit: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Parses arguments, builds the app, and runs it.
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{USAGE}");
            return Ok(());
        }
        Err(message) => return Err(message.into()),
    };

    let backend: Box<dyn Backend> = if let Some(path) = &args.replay {
        Box::new(ReplayBackend::from_path(path)?)
    } else if args.demo {
        Box::new(DemoBackend::default())
    } else {
        match AnthropicBackend::from_env() {
            Ok(backend) => Box::new(backend),
            Err(message) => {
                eprintln!("rabbit: {message}");
                eprintln!("rabbit: using the offline demo backend (pass --demo to silence this).");
                Box::new(DemoBackend::default())
            }
        }
    };

    let app = build_app(&args, backend)?;
    app::run_themed(app, theme_config(&args)).await?;
    Ok(())
}

/// Resolves the theme configuration from the flags and the environment: the
/// `--theme` flag wins, then `RABBIT_THEME`, else the built-in default theme.
fn theme_config(args: &Args) -> app::ThemeConfig {
    let file = args
        .theme
        .clone()
        .or_else(|| std::env::var_os("RABBIT_THEME").map(PathBuf::from));
    app::ThemeConfig { base: None, file }
}

/// Assembles the [`Agent`], resuming or creating a session as the flags direct.
fn build_app(args: &Args, backend: Box<dyn Backend>) -> Result<Agent, Box<dyn std::error::Error>> {
    if let Some(path) = &args.resume {
        let (session, history) = Session::resume(path)?;
        return Ok(Agent::new(args.model.clone(), backend).with_session(session, history));
    }
    if args.continue_latest {
        if let Some(path) = Session::latest()? {
            let (session, history) = Session::resume(path)?;
            return Ok(Agent::new(args.model.clone(), backend).with_session(session, history));
        }
    }
    let session = Session::create(args.model.clone(), now_seconds())?;
    Ok(Agent::new(args.model.clone(), backend).with_session(session, Vec::new()))
}

/// Seconds since the Unix epoch, for stamping a fresh session file.
fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Parsed command-line arguments.
struct Args {
    /// The model id to request.
    model: String,
    /// A specific session file to resume.
    resume: Option<PathBuf>,
    /// Whether to resume the most recent session.
    continue_latest: bool,
    /// A JSONL replay fixture to play instead of calling the API.
    replay: Option<PathBuf>,
    /// Whether to force the offline demo backend.
    demo: bool,
    /// A TOML theme file to load (overrides `RABBIT_THEME`).
    theme: Option<PathBuf>,
}

impl Args {
    /// Parses arguments. `Ok(None)` means help was requested (print usage, exit 0).
    fn parse(args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut model = "claude-opus-4-8".to_string();
        let mut resume = None;
        let mut continue_latest = false;
        let mut replay = None;
        let mut demo = false;
        let mut theme = None;
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--model" => model = value(&mut args, "--model")?,
                "--theme" => theme = Some(value(&mut args, "--theme")?.into()),
                "--resume" => resume = Some(value(&mut args, "--resume")?.into()),
                "--continue" => continue_latest = true,
                "--replay" => replay = Some(value(&mut args, "--replay")?.into()),
                "--demo" => demo = true,
                "-h" | "--help" => return Ok(None),
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(Some(Self {
            model,
            resume,
            continue_latest,
            replay,
            demo,
            theme,
        }))
    }
}

/// Reads the value following a flag, erroring if it is missing.
fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}
