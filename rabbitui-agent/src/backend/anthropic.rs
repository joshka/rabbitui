//! The real Anthropic Messages backend.
//!
//! Turns a [`ChatRequest`] into a streaming POST to `/v1/messages`, feeds the
//! response bytes through the [`SseDecoder`](super::sse::SseDecoder), and forwards
//! the decoded [`StreamEvent`]s. The wire shape does not leak past this module —
//! the app sees only `StreamEvent`s, exactly as it does from the replay backend.
//!
//! Auth resolves from the environment: `ANTHROPIC_API_KEY` (sent as `x-api-key`),
//! or `ANTHROPIC_AUTH_TOKEN` (an OAuth bearer, sent as `Authorization: Bearer` with
//! the `oauth-2025-04-20` beta header). Set the latter with
//! `set -a; eval "$(ant auth print-credentials --env)"; set +a` after
//! `ant auth login` — the `set -a` matters: `print-credentials --env` emits bare
//! `KEY=value` with no `export`, so a plain `eval` sets but does not export it,
//! and the process would not see it. The base URL honors `ANTHROPIC_BASE_URL`
//! (default `https://api.anthropic.com`), so a gateway or proxy just works.

use futures_util::StreamExt as _;
use serde_json::json;
use tokio::sync::mpsc;

use super::sse::SseDecoder;
use super::{Backend, BackendError, ChatRequest, EventStream, StreamEvent};

/// The default API base URL, overridable via `ANTHROPIC_BASE_URL` (a gateway,
/// proxy, or a compatible endpoint).
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// The API version header value.
const API_VERSION: &str = "2023-06-01";
/// The per-response output cap; streaming keeps the request from timing out at
/// this size.
const MAX_TOKENS: u32 = 64_000;
/// The system prompt.
const SYSTEM_PROMPT: &str =
    "You are a helpful assistant running inside a terminal chat client. Keep replies focused.";

/// Credentials resolved from the environment.
#[derive(Debug, Clone)]
enum Auth {
    /// An `sk-ant-...` key, sent as `x-api-key`.
    ApiKey(String),
    /// An OAuth bearer token, sent as `Authorization: Bearer` with the OAuth beta.
    Bearer(String),
}

impl Auth {
    /// Resolves credentials, preferring an explicit API key over an OAuth token.
    fn from_env() -> Option<Self> {
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY")
            && !key.is_empty()
        {
            return Some(Auth::ApiKey(key));
        }
        if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN")
            && !token.is_empty()
        {
            return Some(Auth::Bearer(token));
        }
        None
    }

    /// Adds the auth headers to `builder`.
    fn apply(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            Auth::ApiKey(key) => builder.header("x-api-key", key),
            Auth::Bearer(token) => builder
                .header("authorization", format!("Bearer {token}"))
                .header("anthropic-beta", "oauth-2025-04-20"),
        }
    }
}

/// A backend that talks to the Anthropic Messages API.
#[derive(Debug, Clone)]
pub struct AnthropicBackend {
    /// The shared HTTP client.
    client: reqwest::Client,
    /// The resolved credentials.
    auth: Auth,
    /// The full messages endpoint, built from `ANTHROPIC_BASE_URL`.
    endpoint: String,
}

impl AnthropicBackend {
    /// Builds a backend, resolving credentials and the base URL from the
    /// environment.
    ///
    /// # Errors
    ///
    /// Returns a message describing how to authenticate if no credentials are set,
    /// or if the HTTP client cannot be built.
    pub fn from_env() -> Result<Self, String> {
        let auth = Auth::from_env().ok_or_else(|| {
            "no Anthropic credentials found — export ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN \
             (they must be exported, not just set), or run `ant auth login` then \
             `set -a; eval \"$(ant auth print-credentials --env)\"; set +a`"
                .to_string()
        })?;
        let base = std::env::var("ANTHROPIC_BASE_URL")
            .ok()
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let endpoint = format!("{}/v1/messages", base.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| format!("could not build HTTP client: {error}"))?;
        Ok(Self {
            client,
            auth,
            endpoint,
        })
    }
}

impl Backend for AnthropicBackend {
    fn send(&mut self, request: ChatRequest) -> EventStream {
        // A task drives the request and decodes the SSE; the returned stream drains
        // the channel it feeds. An unbounded channel keeps `send` non-blocking.
        let (tx, rx) = mpsc::unbounded_channel();
        let client = self.client.clone();
        let auth = self.auth.clone();
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move {
            run_request(&client, &auth, &endpoint, request, &tx).await;
        });
        Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }))
    }
}

/// Drives one request: POST, check status, stream + decode, forward events.
async fn run_request(
    client: &reqwest::Client,
    auth: &Auth,
    endpoint: &str,
    request: ChatRequest,
    tx: &mpsc::UnboundedSender<Result<StreamEvent, BackendError>>,
) {
    // The `tools` array (slice 4) is declared here; the app runs the tool loop
    // above the backend and re-sends the grown history each continuation.
    //
    // CRITICAL CAVEAT (offline session): the live continuation request's
    // acceptance — especially thinking-block replay (the API requires the
    // `Thinking{thinking, signature}` block returned unmodified on the turn
    // after a `tool_use` stop) — can only be verified against the real endpoint,
    // which is unreachable here. The SHAPE is fully offline-tested (wire
    // serialization round-trips + a replay-fixture reducer/modal/executor flow);
    // the live tool-continuation smoke test is PENDING a real key. See
    // `docs/design/arc3-slice4-tools.md`.
    let body = json!({
        "model": request.model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": SYSTEM_PROMPT,
        "thinking": { "type": "adaptive", "display": "summarized" },
        "tools": crate::tools::declarations(),
        "messages": request.messages,
    });

    let builder = client
        .post(endpoint)
        .header("content-type", "application/json")
        .header("anthropic-version", API_VERSION);
    let builder = auth.apply(builder).json(&body);

    let response = match builder.send().await {
        Ok(response) => response,
        Err(error) => {
            let _ = tx.send(Err(BackendError::Transport(error.to_string())));
            return;
        }
    };

    let status = response.status();
    if !status.is_success() {
        let raw = response.text().await.unwrap_or_default();
        let _ = tx.send(Err(BackendError::Api {
            status: status.as_u16(),
            message: error_message(&raw),
        }));
        return;
    }

    let mut bytes = response.bytes_stream();
    let mut decoder = SseDecoder::new();
    // Accumulate raw bytes so a multi-byte character split across chunks is decoded
    // whole: feed only the valid UTF-8 prefix, keep the incomplete tail.
    let mut carry: Vec<u8> = Vec::new();
    while let Some(chunk) = bytes.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = tx.send(Err(BackendError::Transport(error.to_string())));
                return;
            }
        };
        carry.extend_from_slice(&chunk);
        let valid = match std::str::from_utf8(&carry) {
            Ok(text) => text.len(),
            Err(error) => error.valid_up_to(),
        };
        if valid == 0 {
            continue;
        }
        let text: String = String::from_utf8_lossy(&carry[..valid]).into_owned();
        carry.drain(..valid);
        let mut events = Vec::new();
        decoder.push(&text, &mut events);
        for event in events {
            if tx.send(event).is_err() {
                return; // the app dropped the stream (e.g. cancelled the turn).
            }
        }
    }
}

/// Extracts a human-readable message from an API error body, falling back to the
/// raw text.
fn error_message(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            if raw.is_empty() {
                "request failed".to_string()
            } else {
                raw.to_string()
            }
        })
}
