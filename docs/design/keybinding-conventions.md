# Keybinding conventions — the common layer

Written 2026-07-11 (Fable pass; author-requested). Terminal users arrive with decades of
muscle memory from readline, Emacs, vim, CUA desktops, and pickers like fzf. A framework
that honors those expectations by default feels native; one that doesn't makes every app
author rediscover them. This doc is the reference layer: what the widgets honor built-in,
what apps should adopt, and — critically — what the terminal can even deliver. Work items
at the end are the implementable spec.

## 1. Editing keys (readline/Emacs) — `TextInput`'s contract

The single strongest convention set in existence: every shell, REPL, and prompt honors
these. `TextInput` should handle them internally (widget `handle`, no keymap involvement).

| Chord              | Action                     | Tier     |
| ------------------ | -------------------------- | -------- |
| Ctrl-A / Home      | cursor to start of line    | must     |
| Ctrl-E / End       | cursor to end of line      | must     |
| Ctrl-B / Left      | cursor left one char       | must     |
| Ctrl-F / Right     | cursor right one char      | must     |
| Alt-B / Ctrl-Left  | cursor left one word       | must     |
| Alt-F / Ctrl-Right | cursor right one word      | must     |
| Ctrl-W             | delete word before cursor  | must     |
| Alt-D              | delete word after cursor   | should   |
| Ctrl-U             | kill to start of line      | must     |
| Ctrl-K             | kill to end of line        | must     |
| Ctrl-H / Backspace | delete char before cursor  | must     |
| Ctrl-D / Delete    | delete char under cursor   | must (1) |
| Ctrl-T             | transpose chars            | may      |
| Ctrl-Y             | yank last kill             | may (2)  |

1. Ctrl-D on an _empty_ input traditionally means EOF/close — apps may bind it (the
   flagship and log-follower use Ctrl-D to close modals); the widget should only consume
   it when the input is non-empty, so the app-level meaning stays reachable.
2. Yank implies a kill ring; a single-slot kill buffer (last `Ctrl-W`/`U`/`K` result) is
   the honest v1 — full ring is out of scope.

Multi-line composers (the flagship) add: Alt-Enter/Shift-Enter newline (terminal
permitting — see §5), Up/Down move within wrapped lines before falling through to history.

## 2. Navigation keys (CUA / OS conventions) — lists, tables, scroll views

| Chord              | Action                        | Tier   |
| ------------------ | ----------------------------- | ------ |
| Up / Down          | move selection                | must   |
| Home / End         | first / last item             | must   |
| PageUp / PageDown  | page selection or viewport    | must   |
| Ctrl-Home/Ctrl-End | document start / end          | should |
| Tab / Shift-Tab    | focus next / previous (3)     | must   |
| Enter              | activate                      | must   |
| Space              | toggle (checkbox, selection)  | must   |

Note (3): focus traversal is framework-owned (routing's Tab/BackTab defaults); apps and widgets
never rebind it.

macOS notes: Cmd never reaches a terminal app (the emulator owns it); Option-as-Meta is a
terminal-emulator _setting_ — when off, Option-arrows send escape sequences most terminals
map to word-jump anyway. Treat Alt-B/F and Ctrl-Left/Right as the same logical action.

## 3. Modal-idiom presets (opt-in, guarded)

Printable-key bindings conflict with text inputs; every printable chord below relies on
the consumed-guard (`Chord::is_guarded` / `action_for_guarded`) that already protects
typing. Offer these as _documented opt-in_ sets, not defaults:

- **vim-ish list nav**: `j`/`k` down/up, `g`/`G` first/last, `Ctrl-D`/`Ctrl-U` half-page,
  `/` filter-focus. (Half the terminal audience expects these in any list app: less, lazygit,
  k9s.)
- **Emacs-ish alternates**: `Ctrl-N`/`Ctrl-P` down/up (also the fzf idiom), `Ctrl-V` /
  `Alt-V` page (conflicts with paste expectations — prefer Ctrl-N/P only).
- **Picker/fzf idiom** (for filtered lists): type-to-filter always live, `Ctrl-N`/`Ctrl-P`
  or Up/Down move, Enter accepts, Esc cancels, Tab multi-selects.

## 4. App-level conventions (recommended defaults for every rabbitui app)

| Chord     | Action                     | Note                                       |
| --------- | -------------------------- | ------------------------------------------ |
| Ctrl-C    | quit                       | the TUI convention; text inputs pass it    |
| q         | quit (guarded)             | browse-mode apps; guarded by consumed()    |
| ?         | help overlay (guarded)     | strongest help convention: less, vim, gh   |
| Ctrl-G    | help/cancel alias          | works everywhere; the flagship's alias     |
| Esc       | dismiss topmost layer      | see §5 lone-Esc caveat                     |
| Ctrl-Z    | suspend                    | wire via Wave D (qwertty ships SIGTSTP)    |
| Ctrl-L    | force full repaint         | the terminal-damage recovery convention    |

The flagship's `Ctrl-/` help chord is a good _additional_ alias but terminals disagree on
what Ctrl-/ sends (often `0x1f`, sometimes nothing) — never make it the only binding.

## 5. What the terminal can actually deliver (the constraints layer)

Legacy terminal encoding aliases keys; bindings must respect these or silently break:

| You bind             | The terminal sends        | Consequence                                 |
| -------------------- | ------------------------- | ------------------------------------------- |
| Ctrl-I               | same byte as Tab (0x09)   | never bind both; Tab wins (focus traversal) |
| Ctrl-M               | same byte as Enter (0x0D) | never bind both; Enter wins                 |
| Ctrl-[               | same byte as Esc (0x1B)   | never bind both; Esc wins                   |
| Ctrl-H               | often Backspace (0x08)    | treat as Backspace unless kitty says else   |
| Ctrl-Space           | NUL (0x00)                | usually deliverable; test per terminal      |
| Alt-x                | ESC prefix + x            | collides with lone-Esc detection (below)    |
| Shift/Ctrl-Enter (4) | nothing (legacy)          | only exist under kitty keyboard protocol    |
| Ctrl-Tab             | nothing (legacy)          | emulator keeps it; don't bind               |

Note (4): likewise Ctrl-Shift-anything and other modifier combinations the legacy encoding
cannot express.

- **Lone Esc** is ambiguous with escape-sequence prefixes; decoders disambiguate by
  timing. qwertty 0.1.x ships lone-Escape flush timing control (Wave D4 adopts it) —
  until then, prefer a second non-Esc binding for anything Esc closes (the flagship's
  Ctrl-G pattern).
- **Kitty keyboard protocol** is the unlock for everything in the "nothing (legacy)" row —
  qwertty negotiates it (verify-after-push) and reports it in `Capabilities`. Convention:
  bind enhanced chords as _additional_ bindings, never the only one; check capability
  before advertising them in the help overlay.
- **tmux** swallows or rewrites some enhanced sequences per version; treat capability
  evidence (qwertty's probed/inferred distinction) as the truth, not $TERM.
- **Bracketed paste**: a paste must never execute bindings (a pasted `q` must not quit).
  qwertty reports paste mode; the framework routes paste bursts as text, not chords —
  keep this invariant tested.

## 6. Work items (the implementable spec)

1. **TextInput readline coverage** — implement §1's must+should tiers in
   `rabbitui-widgets/src/text_input.rs` `handle` (grapheme-correct word motion using the
   same segmentation the widget already uses; single-slot kill buffer). Tests per chord,
   plus the Ctrl-D empty-input pass-through rule. _Small, high value, no dependencies._
2. **List/Table navigation** — ensure `SelectionList` (and Wave B2's `Table`) cover §2's
   must tier (Home/End/PageUp/PageDown exist in the Table spec; verify SelectionList).
3. **`presets` module in `rabbitui-core::keymap`** — named `Chord` constants + documented
   binding-set builders for §3/§4 (`presets::vim_list()`, `presets::picker()`,
   `presets::app_defaults()` returning `&[Chord]`/helper structs an app maps to its own
   action enum — presets cannot be `Binding<A>` for an app's `A`; they provide chords and
   docs, the app supplies actions).
4. **Help-overlay integration** — `HelpOverlay::from_keymap` already renders bindings;
   add capability-aware filtering (hide kitty-only chords when unsupported) once Wave D
   lands capabilities plumbing.
5. **Paste-never-executes-bindings test** — an e2e over the FakeDevice harness feeding a
   bracketed-paste burst containing `q` and asserting the app did not quit.
6. This doc is the acceptance reference: each item cites the section it implements.
