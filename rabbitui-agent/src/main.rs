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
use rabbitui_agent::backend::replay::ReplayBackend;
use rabbitui_agent::demo::DemoBackend;
use rabbitui_agent::session::Session;

const USAGE: &str = "\
rabbit — a terminal chat/agent client (rabbitui flagship)

USAGE:
    rabbit [OPTIONS]

OPTIONS:
    --model <ID>      Model id to request (default: claude-opus-4-8)
    --continue        Resume the most recent session
    --resume <FILE>   Resume a specific session file
    --replay <FILE>   Use a JSONL replay fixture instead of the demo backend
    -h, --help        Print this help
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

    let backend: Box<dyn Backend> = match &args.replay {
        Some(path) => Box::new(ReplayBackend::from_path(path)?),
        None => Box::new(DemoBackend::default()),
    };

    let app = build_app(&args, backend)?;
    app::run(app).await?;
    Ok(())
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
    /// A JSONL replay fixture to use instead of the demo backend.
    replay: Option<PathBuf>,
}

impl Args {
    /// Parses arguments. `Ok(None)` means help was requested (print usage, exit 0).
    fn parse(args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut model = "claude-opus-4-8".to_string();
        let mut resume = None;
        let mut continue_latest = false;
        let mut replay = None;
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--model" => model = value(&mut args, "--model")?,
                "--resume" => resume = Some(value(&mut args, "--resume")?.into()),
                "--continue" => continue_latest = true,
                "--replay" => replay = Some(value(&mut args, "--replay")?.into()),
                "-h" | "--help" => return Ok(None),
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(Some(Self {
            model,
            resume,
            continue_latest,
            replay,
        }))
    }
}

/// Reads the value following a flag, erroring if it is missing.
fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}
