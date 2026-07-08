# Synchronized output (mode 2026) is undetectable/unforwarded inside a multiplexer

> **DRAFT — pending author review. Not filed. See `README.md` in this directory.**

_Proposed title:_ Synchronized output (mode 2026) is undetectable and unforwarded inside a
multiplexer, so apps flicker even when the outer terminal supports it.

## Summary

Synchronized output (DEC private mode 2026) is one of the recent terminal extensions that deployed
cleanly: an application brackets a multi-step repaint in a begin/end pair and the terminal presents
it atomically, so a resize or a multi-write repaint never shows a half-drawn frame. It degrades to a
no-op on terminals that do not implement it, which is exactly the property that makes it safe to emit.
The gap is in the middle box: inside a multiplexer, an application cannot reliably **detect** that
mode 2026 is available, and in some configurations the begin/end sequences are not **forwarded** to
the outer terminal — so the application either flickers (it did not detect support and did not emit
the framing) or emits framing that is swallowed (it detected support that the multiplexer does not
actually deliver).

## Minimal reproduction

1. Use an outer terminal that supports mode 2026.
2. Run a multiplexer inside it (one whose forwarding/support for 2026 is incomplete or version-gated).
3. Run, inside that, an application that repaints a live region each frame and brackets each repaint
   in `CSI ? 2026 h` … `CSI ? 2026 l`.
4. Trigger rapid repaints (stream output, or resize the window).

## Expected

The application queries mode 2026 once at startup (via DECRQM), gets a truthful answer for the
**composite** it is actually talking to (the minimum of what the multiplexer forwards and what the
outer terminal supports), and — when the answer is "supported" — its begin/end framing reaches the
presenting terminal so repaints are atomic and flicker-free.

## Actual

- The startup DECRQM query for 2026 is not answered by the multiplexer, or is answered for the
  multiplexer's own capability rather than the composite, so the application cannot tell whether
  emitting the framing will help.
- In configurations where the multiplexer does not forward the begin/end sequences, framing the
  application does emit is dropped, and the repaint flickers anyway — the exact failure a shipped CLI
  reported as flicker under a multiplexer that otherwise supports 2026
  ([anthropics/claude-code#37283](https://github.com/anthropics/claude-code/issues/37283)).

This is the same class of middle-box failure that affects other queries: an OSC 11 background-colour
query sent inside a multiplexer is neither forwarded nor answered and simply times out
([openai/codex#19741](https://github.com/openai/codex/issues/19741)); the kitty keyboard activation
sequence is swallowed so an application never receives it through the multiplexer
([anthropics/claude-code#26629](https://github.com/anthropics/claude-code/issues/26629)). The pattern
is that a capability the outer terminal genuinely has is invisible or unreachable through the
multiplexer.

## Proposed direction

Specify multiplexer behaviour for mode 2026 **normatively**, following the discipline of the
extensions that worked (query, event, degrade, forward, bounded state):

- The multiplexer MUST answer the DECRQM query for 2026 for the **composite** it presents — reporting
  supported only if it will actually forward (or itself honour) the framing. "The middle box answers
  for the composite it delivers" should be part of conformance, not folklore.
- When the multiplexer implements 2026 itself, it re-encodes the framing inward and presents the pane
  atomically; when it does not, it forwards the begin/end pair verbatim to the outer terminal.
- A swallowed query that leaves the application unable to distinguish "unsupported" from "not
  answered" should be treated as a defect, because silence is ambiguous and forces timing heuristics.

## Degradation

None of this changes the single-terminal case: an application that queries and gets "unsupported"
emits no framing and behaves exactly as today. The ask is narrowly that the _composite_ answer be
truthful and the framing reach the presenter when the answer is "supported."
