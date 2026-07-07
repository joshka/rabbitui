# Arc 3 plan — the flagship agent client

The flagship is an Anthropic-API-backed agent/chat client, grown out of `examples/agent.rs` into its
own workspace binary crate, maintained permanently as the framework's acceptance test. It exists
because its requirements _are_ the library's pitch: streaming text into an inline scrollback,
tool-call cells with live status, modal confirmation over a transcript, resumable sessions, a
generated help overlay. Every design decision below was made with the wire-format reference loaded
(the claude-api skill, 2026-07 revision); where a wire detail is asserted here, trust it over
training priors.

## Fixed decisions

- **Crate:** new workspace member `rabbitui-agent` (binary). Binary name is `rabbit` as a
  placeholder — **author decides the final name**; keep it a one-line change in `Cargo.toml`.
  `examples/agent.rs` stays as the small demo; the crate starts as a copy and diverges.
- **Backend abstraction:** everything network-shaped sits behind one trait so the TUI is testable
  offline and the e2e tapes never touch the network:

  ```rust
  trait Backend {
      fn send(&mut self, request: ChatRequest) -> BoxStream<'_, Result<StreamEvent, BackendError>>;
  }
  ```

  Two implementations: `AnthropicBackend` (real wire) and `ReplayBackend` (plays JSONL fixture
  files; also a `record` mode on `AnthropicBackend` that tees raw SSE lines to a fixture for later
  replay). `StreamEvent` is our own enum (TextDelta, ThinkingDelta, ToolUseStart/Delta/Stop,
  MessageDone{stop_reason, usage}, …) — the SSE wire shape does not leak past the backend module.

- **HTTP + SSE:** Rust has no official Anthropic SDK, so raw HTTP is the sanctioned path. Use
  `reqwest` (features: `rustls-tls`, `stream`, `json` — **no default features**, avoiding openssl)
  plus `futures-util`; parse SSE by hand (split on `\n`, track `event:`/`data:` pairs, dispatch on
  the `data:` JSON's `type` field). Do not add an eventsource crate — the format is trivial and the
  hand-rolled parser is unit-testable against recorded fixtures.
- **Wire shape** (POST `https://api.anthropic.com/v1/messages`):
  - Headers: `content-type: application/json`, `anthropic-version: 2023-06-01`, and auth:
    `x-api-key: $ANTHROPIC_API_KEY` when that env var is set, else
    `Authorization: Bearer $ANTHROPIC_AUTH_TOKEN` **plus** `anthropic-beta: oauth-2025-04-20`
    (tokens from `ant auth print-credentials --access-token`). If neither var is set, print a
    friendly startup error naming both options — never prompt for a key in the TUI.
  - Body: `model` (default `claude-opus-4-8`, overridable via `--model`/config), `max_tokens:
64000`, `stream: true`, `thinking: {"type": "adaptive", "display": "summarized"}` (summarized
    is an explicit opt-in — the default is omitted/empty thinking text, and we render thinking, so
    opt in), `system` (small fixed string), `messages` (full history each turn — the API is
    stateless). No `temperature`/`top_p` (removed on 4.7+; never send). No assistant prefill
    (400s on 4.6+).
  - SSE events to handle: `message_start`, `content_block_start`, `content_block_delta`
    (`text_delta`, `thinking_delta`, `input_json_delta`), `content_block_stop`, `message_delta`
    (carries `stop_reason` + usage), `message_stop`, `ping`, `error`.
  - `stop_reason` handling: `end_turn` normal; `max_tokens` → render a truncation notice cell;
    `refusal` → render a notice cell with `stop_details.category` if present (no fallback chain —
    that's a Fable-only concern and we default to Opus); `tool_use` → run the tool loop (slice 4);
    `pause_turn` → re-send with the assistant content appended, no extra user text, capped at 5
    continuations.
  - Errors: map 401/403/429/413/5xx/529 to a typed `BackendError`; honor `retry-after` on 429 with
    a visible countdown cell; 5xx/529 retry twice with backoff; everything renders as an error cell
    in the transcript, never a crash or silent drop.
- **Transcript persistence:** JSONL, one file per session under
  `${XDG_DATA_HOME:-~/.local/share}/rabbitui-agent/sessions/<timestamp>.jsonl`. Each line is an
  API-shaped message (so resume = deserialize into `messages` and continue) plus a `meta` first
  line (model, created-at, title from the first user line). `--continue` resumes the latest,
  `--resume <file>` a specific one. Thinking blocks are persisted as received and passed back
  verbatim on the same model (required for multi-turn correctness).
- **Rendering:** inline mode by default (the whole point: committed turns become terminal
  scrollback via append-once commits; the composer + streaming tail live in the bounded live
  region). Alt-screen browse mode on Ctrl-B for scrolling history with the ScrollView. Streaming
  markdown is **source-stored, render-wrapped**: accumulate deltas in the message source, re-render
  the tail each frame, commit completed blocks (paragraph/fence boundaries) as `Vec<Span>` lines.
- **Markdown scope** (built in-app first, extracted later — toolong's lesson inverted): headings,
  bold/italic/strikethrough, inline code, fenced code blocks (no syntax highlighting in v1 beyond
  the existing qwertty tokenizer where trivially applicable), unordered/ordered lists,
  blockquotes. Explicitly out: tables, images, footnotes; links render as `text (url)`. Parser
  decision: hand-rolled line-oriented parser in the app (we own wrapping and streaming-partial
  states; pulldown-cmark's event model fights block-at-a-time commits) — revisit only if the
  hand-rolled one exceeds ~500 lines.
- **Tools (slice 4):** two deliberately safe, local, read-only tools — `read_file` (path confined
  to the cwd subtree, canonicalize + prefix check) and `list_dir` (same confinement) — declared
  with JSON schemas in the request. Every tool call raises a confirmation modal (allow/deny; deny
  sends `is_error: true` result with a "user denied" message). Parallel `tool_use` blocks: execute
  all, return **all results in one user message**. This slice exists to exercise modal-over-
  transcript routing and live tool-call status cells, not to build an agent harness.
- **Keymap (pulls Arc 4's keymap item early):** a declarative `Keymap` table (action → chords)
  drives both dispatch and a generated help overlay (`Ctrl-/`). Ctrl-chords only for app actions
  while the composer is focused (lone Esc is dead upstream; printable keys belong to the composer —
  both are standing invariants). Build the minimal version the app needs in-app; the
  generalization to the framework is Arc 4's item.
- **Theme end-to-end:** `--theme <file.toml>` and a config-file default exercise the facade TOML
  path; ship one bundled example theme file in the crate.

## Slice order (each ends green: check/clippy/test/tapes; each gets a design note)

1. **Extraction + replay + persistence.** Crate skeleton, `Backend` trait, `ReplayBackend`,
   transcript persistence/resume, the existing agent example UI ported. TestApp-driven tests for
   compose→send→stream→commit against replay fixtures. _No network code yet._

   _Refinement adopted during the slice-1 build (2026-07-07):_ the reducer is factored into a pure
   `apply_message(&mut Agent, Msg)` / `on_submit` that only mutate state — `TestApp` does not run
   the real `update` closure, so purity is what makes it testable. Scrollback commits are decoupled
   from state via an `Agent.committed: usize` marker (the update closure commits any not-yet-
   committed _final_ cells in inline mode after the reducer runs), because commits go to native
   scrollback that `TestApp`'s buffer does not model — tests therefore render in alt-screen mode and
   assert the buffer. Tool events are **deferred to slice 4** as the plan intends: slice 1 handles
   `TextDelta` (prose) and `ThinkingDelta` (accumulated, committed as a Muted cell — the fancy
   collapsible is slice 3) and closes the turn on `MessageDone`; the `StreamEvent` tool variants
   exist for enum stability but slice-1 fixtures don't emit them, and the reducer treats a stray
   `stop_reason: tool_use` as `end_turn`. The binary ships a built-in demo `ReplayBackend`
   (the ported scripted response as `StreamEvent`s) so `cargo run --bin rabbit` works before slice 2
   lands the real wire and makes `AnthropicBackend` the default. Composer clear uses
   `update.widget::<TextInput>(path, |s| s.clear())` (the newer API), not the example's re-keying.
2. **Anthropic wire.** `AnthropicBackend`, SSE parser (unit-tested on recorded fixtures), auth
   resolution, error/stop_reason cells, record mode. Record 2–3 real fixtures (short exchanges),
   scrub nothing secret into them (no keys in fixtures), commit them for CI.
3. **Markdown.** Parser + streaming block-commit rendering; thinking cells as collapsed-by-default
   Collapsible (Muted body); code fences as Panel-backed blocks.
4. **Tools.** Tool declarations, confirmation modal, status cells (pending→running→done/error),
   parallel-call handling, `pause_turn` loop.
5. **Chrome.** Keymap + help overlay, theme file flag, alt-screen browse mode, polish pass against
   the Arc 2A bar (spacing tokens, role audit).
6. **Extraction + e2e.** Move Markdown and Modal/Menu widgets into `rabbitui-widgets` (now proven);
   betamax tape suite running against `ReplayBackend` (env var `RABBIT_BACKEND=replay:<fixture>`)
   so tapes are deterministic and network-free; screenshots into the pipeline.

## Delegation notes

Slices 1–2 are sequential and design-bearing: drive them in-session or with a single opus agent per
slice, briefed with this file. Slices 3 and 4 are independent after 2 and can run as parallel opus
agents (separate crates/files; markdown touches the app render path, tools touch update/backend —
agree the `Cell` enum shape first so they don't collide). Slice 6's widget extraction is
sonnet-grade. Standing agent rules from the playbook apply (no jj, durable summaries, decisions
pre-made).

## Acceptance for the arc

A user with `ANTHROPIC_API_KEY` set runs the binary, has a streamed markdown conversation with
tool use and confirmation, quits, runs `--continue`, and the session resumes — all inline, with
scrollback intact, under any of the five themes; CI runs the full tape suite offline via replay.
