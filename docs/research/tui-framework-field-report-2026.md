# Terminal UI Frameworks in 2026: A Field Report

Audience: maintainers of terminal UI libraries, users who work across ecosystems, and teams
choosing a terminal UI stack in 2026.

## Executive Summary

The terminal UI field is no longer short on architectures. It has immediate-mode renderers,
retained trees, Elm loops, React reconcilers, CSS engines, flexbox ports, native compositors,
single-language stacks, foreign-framework ports, and dozens of fresh experiments. The repeated
failure mode is not lack of imagination. It is that architecture alone does not become a product.

The libraries people actually adopt solve one or more of these concrete jobs:

- They make the first useful app small enough to write.
- They make focus, input, resizing, Unicode, and mouse behavior correct enough to trust.
- They ship the widgets users expected not to build themselves.
- They give maintainers a way to test, replay, and inspect behavior without staring at a terminal.
- They render without flicker in both fullscreen and scrollback-preserving modes.
- They provide a styling and theme story that makes applications look deliberate by default.

The research points to a blunt conclusion: the next durable terminal UI layer will be judged less
by its favorite programming paradigm than by its boring operational coverage. Event loop, focus,
forms, overlays, text input, virtualization, theming, inline rendering, terminal cleanup, and
headless tests are the actual table stakes.

## The Field By Library

### Ratatui

Ratatui is the best rendering assembly language in the Rust terminal ecosystem. Its cell buffer,
styled text types, double-buffer diffing, and buffer-level tests are the parts everyone wants to
stand on. It also succeeds because it is honest about what it is not. It does not own input, focus,
event routing, app state, mouse hit-testing, or a runtime.

That refusal is both its strength and its map of missing territory. Every serious Ratatui app
rebuilds the same scaffolding: action channels, focus state, component routing, event loops,
timers, overlays, and text editing. The ecosystem around it is not random fragmentation. It is a
negative-space drawing of the framework Ratatui deliberately does not try to be.

For maintainers, the lesson is to protect the substrate. The buffer and core trait surface are
valuable precisely because they are stable, small, and broadly useful. For users, the lesson is to
choose Ratatui when you want control and can afford to build or adopt the missing middle yourself.
It is an excellent base for custom applications; it is not, by itself, a batteries-included app
framework.

### Textual

Textual is the strongest proof that a retained DOM, stylesheet language, compositor, workers,
screens, focus, and test driver can make terminal apps feel like product software. It is the most
complete answer to "what would a terminal framework look like if it took application ergonomics
seriously?"

Its great success is integration. Focus is not a utility function; it falls out of persistent
widgets. Styling is not each widget author's private problem; selectors, pseudo-classes, variables,
and live reload make third-party widgets themeable. Testing is not an afterthought; a headless
pilot and visual snapshots make behavior reviewable. Workers encode common async failure modes, such
as stale responses, into named framework semantics.

Its caution is centralization. A monolithic package with a large widget set can become a bottleneck
when core widgets need deep specialization. The DataTable performance story and large-log viewer
workarounds show that "included" is not enough; data-heavy widgets must be virtualized and backed by
pluggable data models. Textual also shows that users love CSS-like power, but punish CSS-like names
when terminal semantics diverge from browser expectations.

For users choosing in 2026, Textual remains the most product-complete Python option. For maintainers
elsewhere, its deeper lesson is not "copy CSS." It is "make focus, styling, testing, screens,
workers, and compositor behavior one coherent system."

### Bubble Tea

Bubble Tea is the most teachable terminal UI programming model in the field. `Init`, `Update`,
`View`, and commands that return messages are small enough to fit in a user's head. It turns async
work into a disciplined loop: effects run outside the model, results come back as messages, and
state changes serialize through one update function.

That simplicity breaks at composition. Once an app has several child models, routing messages,
tracking focus, sharing state, sequencing commands, and knowing which child owns an async reply all
become userland problems. The framework gives a beautiful loop, then leaves every serious app to
rebuild a component runtime inside it.

Bubble Tea's v2 shift is revealing. Moving terminal state, keyboard enhancement, cursor state, and
rendering into a structured view acknowledges that strings were too weak as the core abstraction.
The Elm loop remains valuable, but rendering, layers, terminal modes, and cell state need richer
data than "return a string."

For users, Bubble Tea is still a strong Go choice when the app fits the model or the Charm ecosystem
already covers the needed controls. For maintainers, it proves that a tiny runtime contract is worth
stealing, but it must be paired with framework-owned identity, focus, layout, and routing before
composition scales.

### Cursive

Cursive proves retained trees in Rust are possible. It also shows the scars of making them possible
without a modern identity and mutation model.

Its best idea is deferred mutation. A widget handles an event while borrowed by the tree, then
returns a callback or command to run later against the whole application after the borrow ends. That
is not a hack; it is one of the cleanest answers to Rust's "widget wants to mutate app" problem.

The painful parts are the workarounds around identity and access. String names, tree walks,
downcasts, per-node locks, runtime borrow failures, and `Send + Sync` pressure are not incidental
details. They are what happens when a retained tree has no typed, framework-owned identity system.

Cursive remains attractive for form-heavy apps that want a traditional retained toolkit. Its larger
lesson is sharper: retained mode is not disqualified in Rust, but it must be built around typed IDs,
arena ownership, mediated mutation, and a mailbox for external events. If a framework exposes locks
and string selectors as the normal way to reach widgets, users will eventually feel the machinery.

### Brick

Brick is the most elegant pure-functional terminal framework in the survey. Its resource-name
system is a gem: one user-defined name type keys scroll state, extents, cache entries, clickable
regions, cursor candidates, and viewport state. That single identity namespace answers a whole class
of questions immediate-mode frameworks tend to dodge: where did this widget end up, what did the
user click, what scroll offset belongs to this viewport, and which cached render is still valid?

Brick's render result is also instructive. A widget does not only produce pixels. It produces an
image plus cursor candidates, visibility requests, extents, border metadata, and other facts that
containers can translate and bubble upward. That richer composition currency is one of the clearest
ideas in the research.

The tradeoff is performance and layout ceiling. Full redraws and simple Fixed/Greedy layout are
pleasant until large, variable-height, scrollable collections enter the picture. Brick's efficient
path assumes constraints that real data widgets often violate. Its Unicode-width caveats are also a
reminder that terminal rendering correctness depends on agreement with the terminal, not just a
library's local width table.

For maintainers, Brick's resource names and render facts are worth studying closely. For users, it
is still an excellent Haskell choice when its model fits. The portable lesson is that identity
should be a first-class type, not a pile of side channels.

### Ink

Ink won the developer-experience argument and lost the rendering-substrate argument. React
components, hooks, context, flexbox, and test snapshots are exactly why major JavaScript CLIs chose
it. Even teams that replaced the renderer often kept the component model. That is the strongest
possible evidence that the authoring surface is good.

The renderer is the warning. Full React render, full layout, full paint, frame string serialization,
then line or string diffing means most of the work happens before the diff can save anything. Inline
output taller than the viewport creates flicker and scrollback corruption that cannot be patched
away with better string erasure. Large streaming agent CLIs exposed this over and over.

Ink's `<Static>` component is the major idea to preserve. It separates append-only history committed
to real terminal scrollback from a bounded live region that can be repainted. For modern AI-agent
and transcript-heavy CLIs, that split is not a trick. It is the primary screen model.

For users, Ink is still compelling when JavaScript and React are the real requirements, especially
for smaller CLIs. For maintainers, its lesson is precise: keep the component ergonomics, but render
to a real cell model, design inline mode as a first-class target, and never make scrollback
integrity an application author's hidden burden.

### libvaxis and vxfw

libvaxis sets the bar for a modern terminal substrate. Its most important stance is that terminal
capabilities should be queried, negotiated, and represented as runtime facts. Keyboard protocol,
synchronized output, grapheme width, color queries, in-band resize, and multiplexer behavior cannot
be reduced to a stale terminal-name database.

The negotiated-width lesson is especially important. Width is not a property a framework can decide
in isolation. It is a contract between app and terminal. If layout, cursor movement, and rendering
do not use the same width policy the terminal uses, correctness degrades one cell at a time.

The framework layer, vxfw, shows that a great substrate does not automatically make a great
framework. Pointer identity, dangling widget references, widget-level Unicode bugs, and incomplete
integration testing are reminders that the toolkit layer has its own hard problems. The substrate
can make correct input and output possible; it cannot by itself solve ownership, identity,
virtualization, or widget contracts.

For maintainers, the transferable substrate bar is query routing, capability structs, synchronized
frames, panic-safe cleanup, negotiated width, in-band resize, and testable parsing. For users, the
practical lesson is to ask not only "does this framework draw?" but also "does it know what terminal
it is drawing to?"

### OpenTUI

OpenTUI is one of the clearest greenfield signals: a retained scene graph over a native cell-buffer
core, with thin adapters for multiple frontend paradigms, is a productive shape. React, Solid, and
imperative users can share one render tree when the core is stable and the adapters are thin.

Its strengths are modern and concrete: no-op frame suppression, output backpressure, native hit
testing, renderer-owned selection, z-index, scissor rects, retained renderables, and a text system
designed for streaming rich content. It treats the workloads of current terminal apps, especially
agent-style streaming text, as central rather than exotic.

Its pain comes from boundary placement and unfinished framework policy. A TypeScript tree talking
chattily to a native buffer pays for duplicated state and FFI overhead; the roadmap's move toward a
more native core is an admission that the boundary was drawn too high. Focus is another instructive
gap: a single focused owner exists, but traversal is left to applications, so every example rebuilds
Tab order.

For users, OpenTUI is promising where TypeScript, retained trees, and modern streaming interfaces
matter. For maintainers, its lesson is to put tree, layout, text, hit testing, and buffers in one
coherent core, then let adapter layers provide the preferred authoring syntax.

### The Recent Rust Wave

The 2024-2026 Rust wave is the largest demand survey the terminal UI ecosystem has ever run. Dozens
of new projects, including many AI-assisted ones, independently converged on the same list: event
loop, focus, forms, overlays, text input, theming, MVU or hooks, inline rendering, streaming output,
testing, and prettier defaults.

The convergence matters more than any one project. AI-generated frameworks are noisy adoption
signals, but they are useful desire signals. They reproduce the same community requests because they
are trained on those requests: "make Ratatui easier," "give Rust Bubble Tea," "give Rust Ink,"
"give me Textual," "make inline output correct," "ship the widgets."

The wave also changes the workload. AI-agent CLIs are now a flagship terminal UI category. They need
streaming transcripts, markdown, diff views, tool logs, scrollback preservation, task queues, prompt
lines, and deterministic tests that agents can run. This is not a niche; it is where a large share
of new terminal UI energy is going.

For maintainers, the wave says architecture novelty is cheap. Correct interaction behavior is not.
For users, it says to discount starbursts and download counts in new framework cohorts. Look for
real applications, active maintenance, terminal correctness, tests, and a path to the widgets you
actually need.

### Rust GUI Research

The Rust GUI world has already paid for many mistakes terminal UI authors are tempted to repeat.
Tree ownership, widget identity, traversal, state scoping, and incrementality are Rust problems, not
pixel problems. Arena storage, generational IDs, id paths, framework-owned passes, typed contexts,
and headless harnesses transfer directly to terminal frameworks.

What should not transfer wholesale is the rendering machinery. Terminal UIs do not need GPU scene
graphs, subpixel layout, full text shaping, or browser-scale paint invalidation. A terminal frame is
a grid of cells. Full paint into a fresh buffer plus a correct diff is often simpler and more robust
than a clever damage system.

The best synthesis is selective theft. Take identity, passes, storage, replay, typed mutation, and
tooling from GUI research. Leave behind the machinery whose only purpose is to make pixels,
compositors, and native windows fast.

## Cross-Cutting Findings

### 1. Identity Is The Center Of Gravity

Every advanced feature eventually asks whether a widget has identity across time. Focus, hover,
press state, scroll offsets, cursor placement, async replies, caches, hit regions, overlays,
selection, accessibility, and test inspection all need an answer to "which thing is this?"

Libraries answer this differently:

- Ratatui largely leaves identity to applications.
- Textual uses persistent widget objects.
- Bubble Tea leaves child identity and message ownership to parent models.
- Cursive uses string names and runtime access.
- Brick uses a typed resource-name namespace.
- OpenTUI uses retained renderables.
- GUI systems use arena keys and id paths.

The best 2026 answer is explicit, typed, and framework-visible. Identity should be data the
framework can route through, inspect, and test. It should not be an emergent side effect of object
addresses, string names, or list positions alone.

### 2. The Missing Middle Is Now The Product

The repeated user request is not "give me a cleverer renderer." It is:

- a blessed event loop,
- focus traversal,
- text input,
- forms,
- selection lists,
- modals and overlays,
- scrollable and virtualized collections,
- keyboard shortcuts,
- theme tokens,
- panic-safe cleanup,
- inline and fullscreen render modes,
- test drivers and snapshots.

This is the product surface users feel. A framework can have a brilliant architecture and still
fail if users must assemble these pieces from incompatible crates.

### 3. Rendering Correctness Beats Rendering Cleverness

The most painful rendering failures are not abstract performance problems. They are user-visible
flicker, corrupted scrollback, stale rightmost columns after resize, cursor desync, bad wide
characters, double input, and terminals left in broken modes after a crash.

Correctness requires boring features:

- synchronized frame writes,
- real cell buffers,
- terminal capability negotiation,
- a single input/query router,
- negotiated width policy,
- full repaint escape hatches,
- panic and suspend cleanup,
- resize handling that cannot race silently,
- renderer invariants tested against real terminal behavior.

String diffing and erase/rewrite loops are now known liabilities for large interactive CLIs,
especially when preserving scrollback matters.

### 4. Inline Mode Is No Longer Optional

Fullscreen alternate-screen applications are still important, but modern CLI tools often need the
primary screen. Users want native scrollback, search, copy/paste, transcript history, and output
that survives process exit. Agent CLIs made this demand unavoidable.

Correct inline rendering is not "fullscreen rendering, but smaller." It needs a model that separates
append-only committed history from a bounded live region. The live region must never casually erase
or duplicate scrollback. Frameworks that treat inline mode as a side effect of string printing will
continue to accumulate flicker bugs.

### 5. Text Is Infrastructure, Not A Widget

Editable text, streaming text, tables, wrapping, cursor placement, selection, and hit testing all
depend on the same lower layer: grapheme segmentation, width policy, line breaking, and terminal
capability agreement.

If every widget calls its own width function, the framework already has a correctness bug. Text
measurement must be centralized, capability-aware, and shared by layout, rendering, input editing,
and tests.

### 6. Layout Wants Familiar Defaults, Not Solver Novelty

Across the survey, users rarely ask for a more sophisticated constraint solver. They ask for layouts
that do not break when borders, wrapping, resizing, or lists appear. Flexbox-like vocabulary, simple
linear splits, grids, docked regions, intrinsic text measurement, and exact integer distribution
cover most real terminal apps.

The unsolved work is not naming a layout paradigm. It is integrating text measurement,
virtualization, scrolling, overlays, hit testing, and resize behavior into the same layout facts.

### 7. Styling Must Be Shared Across Widgets

Per-widget styling APIs do not scale. They leave holes wherever a widget author forgot to expose a
color or state. The field shows three viable levels:

- style values passed explicitly,
- semantic theme tokens and widget parts,
- selector/cascade systems with hot reload.

The right starting point for many libraries is semantic tokens plus per-widget part styles. CSS-like
power can be earned later, but the theme model must be cross-widget from the beginning. Users now
expect applications to look intentional, and "pretty by default" is no longer a shallow concern.

### 8. Testing Is A Framework Feature

Buffer snapshots are useful but insufficient. The bar is now:

- headless app drivers,
- synthetic key and mouse input,
- deterministic clocks,
- wait-until-idle semantics,
- frame and semantic snapshots,
- replayable interaction tapes,
- PTY-level tests for terminal behavior,
- human-diffable artifacts.

This is also the trust mechanism for AI-assisted development. Broad widget catalogs are easier to
generate than correct interaction behavior. Replay and terminal-level verification are how a project
proves the latter.

## Guidance For Maintainers

If you maintain a terminal UI library, the research suggests these priorities.

First, say clearly what layer you own. A rendering substrate, terminal protocol layer, widget
toolkit, app framework, and styling system have different responsibilities. Confusion at the layer
boundary creates churn.

Second, make identity explicit before stabilizing focus, async effects, overlays, or tests. If the
identity model is weak, every advanced feature will smuggle in a different one.

Third, ship the missing middle before chasing architecture novelty. A small, boring, correct set of
forms, text inputs, lists, dialogs, overlays, and focus rules is more valuable than a novel
component macro without production controls.

Fourth, treat terminal protocol correctness as product quality. Query routing, synchronized output,
keyboard enhancement, paste aggregation, resize events, panic cleanup, and width negotiation are not
backend trivia. They determine whether users trust the app.

Fifth, design tests as public API. A framework that cannot run a user app headlessly, inject input,
drain the loop, and produce reviewable output is hard to maintain and hard to adopt.

Sixth, make incremental adoption possible. The successful Rust ecosystem pattern is layering, not
wholesale replacement. Users want to adopt one concept at a time and keep escape hatches to the
lower layer.

## Guidance For Users Choosing In 2026

Choose by workload, not by paradigm label.

Use Ratatui when you want Rust, control, a strong rendering core, and you are willing to assemble or
write the app framework pieces.

Use Textual when you want the most complete Python terminal app framework, strong styling, screens,
workers, widgets, and tests, and can accept its ecosystem and performance tradeoffs.

Use Bubble Tea when you want Go, a teachable MVU loop, and the Charm ecosystem fits your app. Be
ready to design composition, focus, and layout policy for larger applications.

Use Cursive when traditional retained widgets and forms matter more than modern async ergonomics,
and when its access and styling model fit the application's size.

Use Brick when Haskell and a pure functional model are desired, especially if typed resource names
and declarative render facts align with the app.

Use Ink when React and the JavaScript ecosystem are the deciding factors, especially for smaller or
well-bounded CLIs. Be cautious with large streaming output and scrollback-preserving interfaces.

Use OpenTUI when TypeScript, retained renderables, streaming text, and a native core are attractive,
while accepting that the ecosystem is still moving quickly.

Evaluate newer Rust frameworks by real proof, not feature lists. Look for a flagship app, focus and
mouse correctness, Unicode behavior, resize behavior, tests, and maintenance history. A framework
that claims every paradigm but cannot demonstrate reliable interaction should be treated as a
prototype.

## The 2026 Selection Checklist

Before adopting a terminal UI framework, ask:

1. Does it own the event loop, or clearly document how the application owns it?
2. How does it represent widget identity across frames?
3. Is focus traversal built in, and can it handle overlays and hidden widgets?
4. Are text input, forms, lists, tables, and dialogs first-party or well-integrated?
5. Does it support both fullscreen and scrollback-preserving inline rendering?
6. Does rendering use a real cell model, synchronized output, and safe terminal cleanup?
7. How does it negotiate keyboard protocol, paste, resize, color, and Unicode width?
8. Can data-heavy widgets virtualize rows, columns, and variable-height content?
9. Is styling shared across widgets through tokens, parts, or selectors?
10. Can tests drive the app headlessly with deterministic input and snapshots?
11. Are examples realistic enough to reveal async, resize, focus, and streaming behavior?
12. Is there a stability policy that separates core protocol churn from widget churn?

## Final Synthesis

The strongest libraries in this field are all incomplete in different, useful ways. Ratatui shows
how powerful a clean rendering substrate can be. Textual shows what product completeness looks like.
Bubble Tea shows how much a tiny loop can teach. Cursive shows retained Rust can work, but mutation
must be mediated. Brick shows identity can unify features cleanly. Ink shows that developer
experience can win adoption even when the renderer struggles. libvaxis shows the modern terminal is
queried and negotiated, not guessed. OpenTUI shows a retained scene graph can support multiple
frontends when the core is real.

The field's next step is not another proof that hooks, Elm, retained trees, or immediate mode can be
made to draw a button. That has been proven many times. The next step is a library that makes the
ordinary hard parts ordinary: focus, text, forms, overlays, inline output, capability negotiation,
virtualization, styling, cleanup, and tests.

The winning terminal UI framework in 2026 will feel less like an architecture demo and more like a
maintenance contract with its users: the common cases are covered, the hard terminal cases are
tested, the escape hatches are honest, and the application code stays smaller than the problem it is
trying to solve.
