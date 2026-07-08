# Arc 3, slice 4 — Tools (the flagship agent client)

Slice 4 grows the agent client from prose-only turns into a tool-using loop:
read-only file tools gated by a confirmation modal, with the model's tool calls
executed locally and the results fed back so the turn continues. See
`docs/plans/arc3-agent-client.md` → "Tools (slice 4)".

## Content-block message model

`ChatMessage.content` is now `Vec<ContentBlock>` (was `String`), where
`ContentBlock` is `Text | Thinking{thinking, signature} | ToolUse{id, name,
input} | ToolResult{tool_use_id, content, is_error}`. The `#[serde(tag =
"type", rename_all = "snake_case")]` serialization matches the Anthropic wire
exactly — an array of `{"type": ..., ...}` blocks — and **that same
serialization is the session-persistence JSONL** (wire-shaped resume).

- `ChatMessage::user`/`assistant` still build a single `Text` block, so existing
  call sites and equality-by-constructor tests are unchanged.
- `assistant_blocks(...)` builds a tool-use assistant turn; `tool_results(...)`
  builds the single user message carrying all results.
- `text()` concatenates only `Text` blocks (titles, the transcript, the demo
  backend).
- `ToolResult.is_error` is `skip_serializing_if` false, so a successful result
  omits `is_error` on the wire (only failures/denials carry it).

## Tools (`tools.rs`)

Two read-only tools, `read_file` and `list_dir`, **confined to the cwd subtree**:
the requested path and the cwd are each canonicalized, and the call is refused
unless the canonical target starts with the canonical cwd. Canonicalization
resolves `..` and symlinks, so `../escape`, an absolute path outside the tree,
and a symlink pointing outside are all rejected (unit-tested). `read_file` caps
output at 64 KiB with a truncation note. `execute`/`execute_in` return
`Result<String, String>` (Ok = result text, Err = error message); `declarations()`
is the JSON-schema `tools` array added to the request body in `anthropic.rs`.

## Streamed tool-use accumulation & thinking replay

The SSE decoder gained `signature_delta` handling, surfaced as a new
`StreamEvent::ThinkingSignatureDelta`. The reducer accumulates `ToolUseStart`
(opens a `PendingToolUse`), `ToolUseInputDelta` (appends raw JSON), and
`ToolUseStop`, plus the thinking signature. On `MessageDone{stop_reason:
ToolUse}` it pushes an assistant `ChatMessage` of blocks `[Thinking(if any),
Text(if any), ToolUse per call]` — **the thinking block's signature is replayed
verbatim** (the wire requires unmodified thinking blocks on the continuation) —
adds Pending Tool cells, and arms `Agent.awaiting` (the "awaiting confirmation"
state).

## Confirmation modal & continuation

The modal is a `Frame::layer` per `examples/form.rs`: a centered focused panel
listing the pending call(s) with Allow / Deny, focus moved in via the
declare-then-focus handshake. Allow (button / Enter / 'y') runs every call via
`tools::execute`, marking cells Pending → Running → Done/Error; Deny (button /
Esc / 'n') marks them Failed with an `is_error` "user denied this tool call"
result. Both build **one** user message of all `tool_result` blocks (parallel
tool_use → all results in one message) and re-send the grown history, looping
the turn until `stop_reason: EndTurn`, capped at 5 continuations.

## What is offline-verified vs. pending a live smoke test

Everything in this slice is driven offline: the content-model wire serialization
round-trips to the exact JSON for each block type (`backend/mod.rs` tests); the
tool executor's confinement/read/list/unknown-tool behavior (`tools.rs` tests);
and the full reducer/modal/executor/continuation flow against a JSONL replay
fixture (`tests/slice4.rs`) — a `tool_use` turn opens the modal, Allow drives the
cells Pending → Running → Done and the continuation request carries the single
`tool_result` user message, then a final assistant reply commits; the Deny path
yields `is_error` results.

**PENDING — live tool-continuation smoke test.** The live continuation request's
acceptance — especially **thinking-block replay** (the API rejecting a modified
or dropped thinking block; the exact `signature` echo) — can only be verified
against the real Anthropic endpoint, which this offline session cannot reach
(the same deferral as slice 2's HTTP client). The continuation is implemented per
the claude-api skill's wire reference and the SHAPE is fully offline-tested; a
live smoke call with the user's key is still owed to confirm the endpoint accepts
the replayed thinking block and the one-user-message tool_result batch.

### Wire details flagged for the coordinator to verify

1. **`signature_delta` event shape.** The decoder reads
   `delta.type == "signature_delta"` with the signature in `delta.signature`
   (mirroring `thinking_delta`'s `delta.thinking`). This is the assumed field
   name/placement; confirm against a recorded stream.
2. **Thinking-block replay position.** The continuation assistant message emits
   `Thinking` first, then `Text`, then `ToolUse` blocks. Confirm the API accepts
   this order and the verbatim `signature`.
3. **`tools` declaration placement.** `tools` is added to the request body
   alongside `thinking`/`system`/`max_tokens`; `input_schema` uses
   `{"type": "object", "properties": {...}, "required": [...]}`. Confirm the
   model actually calls the tools with this schema.
4. **Empty-input tool call.** A `list_dir` call with no `path` streams
   `input: {}` (or an empty/absent block); the reducer parses a malformed/empty
   JSON input to `null` and the tool defaults `path` to `"."`. Confirm the wire
   sends `{}` rather than omitting the input.
