# ADR 0014: Build standalone; defer the ratatui-* shipping decision to ~0.1

- Status: accepted (2026-07-06) — decision deferred by design
- Deciders: joshka + research synthesis

## Context — the forces, with evidence

rabbitui is a framework layer above the ratatui/qwertty substrate. Where it ends up on the
naming/branding map — an unaffiliated `rabbitui` crate family, or shipped as/under the
`ratatui-*` names in the ratatui org — is a positioning question distinct from every
technical ADR (0001–0013). This ADR decides *how and when* that question gets answered, not
what the answer is.

Three forces frame it:

1. **Org gravity compounds; unaffiliated framework layers cap.** The 2024–26 wave is the
   largest demand survey ever run on ratatui's missing framework layer (50+ frameworks in 24
   months, ~⅓ AI-generated — `docs/research/recent-rust-tui-wave.md`). Its adoption pattern
   is stark: layers that sit *on* ratatui or keep an escape hatch to it get real traction
   (Ratzilla 1,401★/269k dl; ratatui-interact 22.6k dl in 6 months; rat-salsa the organic
   workhorse), while from-scratch stacks stall at 0–400★ regardless of quality —
   yeehaw 354★, Anathema 352★, AppCUI-rs 396★ over 3.5 years, bubbletea-rs 276★ then paused,
   rooibos 5★ with the complete "right" feature list (`docs/research/prior-art.md`).
   ratatui itself is the control: 36.2M downloads, 13.5M/quarter, versus 204k for the
   next-most-adopted framework. "Unaffiliated framework layers cap at ~400 stars while org
   gravity compounds" (`recent-rust-tui-wave.md` §Implications) is the sharpest single datum
   pointing at shipping under org names.

2. **The reserved names are the author's own protective placeholders — there is no race.**
   200+ `ratatui-*` crate names were reserved on crates.io in June 2026 by this project's
   author, acting as a ratatui maintainer, as explicitly protective reservations:
   `~/local/ratatui-reservations` holds honest, boring, no-fake-API placeholder crates whose
   sole job is to keep plausible official names from being squatted (README: "It should
   expose no fake API. It should say that it is a Ratatui namespace reservation."). The
   framework-layer names that matter here — `ratatui-runtime`, `-component`, `-state`,
   `-theme`, `-testing`, `-snapshot`, `-profiler`, plus `-action`, `-layout`, `-interaction`,
   `-text`, `-controls`, `-app`, `-framework` — are all reserved. Crucially: because the
   author owns them, the official landing path is *pre-cleared*. "Coordinate with upstream"
   is not a negotiation but a choice this project's author gets to make
   (`recent-rust-tui-wave.md` §3). There is no competitive clock forcing an early call.

3. **The technical work is naming-agnostic and can proceed now.** None of ADRs 0001–0013
   assumes a crate prefix. The cell model is ratatui-compatible by construction (ADR 0003);
   interop is a leaf bridge crate that never touches core (ADR 0010); the crate layout (ADR
   0011) is a clean foundation→family→facade split that renames mechanically. Building can
   start immediately without the positioning question blocking a single line of code.

The cost of deciding *now* is real: committing to `ratatui-*` early couples an unproven core
to org expectations and a maintainer's reputation before the interaction-correctness moat is
demonstrated; committing to standalone-forever early throws away the one lever the evidence
says most reliably moves adoption. Deferring costs almost nothing because the architecture
keeps both doors open.

## Options considered

### A. Commit now to shipping under `ratatui-*` in the ratatui org

*What it is:* declare from v0.0 that the framework lands as `ratatui-runtime`/`-component`/
`-state`/… under the org, and build toward that from the first commit.

*Steelman:* the adoption evidence is one-directional. Org gravity compounds and unaffiliated
layers cap (`recent-rust-tui-wave.md`); "LLM defaults now funnel new projects to ratatui"
(HN 45830829), so an official framework layer inherits the recommendation flywheel that no
standalone name can buy. The names are already the author's; the path is pre-cleared; every
month spent branding a separate `rabbitui` identity is a month of adoption gravity forgone.

*Why not chosen:* it front-loads an irreversible commitment onto an unproven core. rabbitui's
advertised moat is interaction correctness proven at the PTY level (DESIGN.md), and the wave's
lesson is precisely that breadth without demonstrated correctness reads as "broken in a subtle
way" (FrankenTUI, HN 46986644). Stamping the official ratatui name on the framework *before*
that proof exists spends the org's credibility as collateral on a bet not yet won, and an
official crate is far harder to retract or restructure than an experimental one. The evidence
argues the org name is *valuable*, not that it must be claimed on day zero — and nothing about
building standalone forecloses claiming it later.

### B. Commit now to a permanently standalone `rabbitui` identity

*What it is:* decide up front that rabbitui ships as its own crate family, unaffiliated,
interoperating with ratatui only through the bridge (ADR 0010).

*Steelman:* independence is clean. No org process, no coordination, no obligation to match
ratatui's release cadence or governance; the project moves at its own speed and its
architecture (an independent buffer with layers/z-order/inline mode that ratatui structurally
lacks — ADR 0003, 0013) is unencumbered by any expectation of being "the official ratatui
framework." Some users actively prefer an independent tool; one experienced user "quit Bubble
Tea because of inconsistencies... went to ratatui and never looked back" (HN 46798402) —
brand independence lets the project be judged on its own terms.

*Why not chosen:* it discards, permanently and prematurely, the single lever the survey most
strongly associates with escaping the ~400-star ceiling. Every from-scratch stack in the wave
that chose independence is sitting at that ceiling (`prior-art.md`, `recent-rust-tui-wave.md`).
Deciding *now* to never use the org names — when the author owns them and the path is
pre-cleared — is throwing away optionality for no compensating benefit, since the technical
work is identical either way until ~0.1.

### C. Build standalone; defer the shipping/naming decision to ~0.1 (CHOSEN)

*What it is:* develop under the working `rabbitui` name with a naming-agnostic architecture,
and make the final ship-as-`ratatui-*`-or-not call at the ~0.1 milestone, once the core is
proven, with the adoption tradeoff recorded and the option held open by construction.

*Steelman:* it dominates A and B on optionality at near-zero cost. The architecture already
keeps the door open — ratatui-compatible cells (ADR 0003), a bridge crate (ADR 0010), a
mechanically-renamable crate layout (ADR 0011), and no naming assumption anywhere in the API.
The reserved names guarantee the door *stays* open (nobody else can take them). Deferring lets
the interaction-correctness proof — the thing that should decide whether the org name is
*earned* — actually exist before the name is spent. And ~0.1 is the natural decision point: by
then the widget catalog, inline-mode proof, and PTY harness results are real, so the call is
made on evidence rather than ambition. The final call is the author's, made at ~0.1.

*Why not chosen — its honest cost:* deferral defers the adoption flywheel too. If the answer
was always going to be "ship under the org," every month of standalone branding is a month of
org gravity not compounding, and some early `rabbitui`-branded mindshare/URLs/docs become
sunk cost at rename time. This ADR accepts that: the correctness proof is worth more than a few
months of early gravity, and a rename at ~0.1 (pre-1.0, small user base) is cheap.

## Decision

1. **rabbitui builds standalone under the working name `rabbitui`.** Development proceeds
   immediately; the positioning question blocks no technical work.

2. **The ship-as/under-`ratatui-*` decision is deferred to the ~0.1 milestone** and is made
   on evidence available then — interaction-correctness results at the PTY level, catalog
   coverage, inline-mode proof, and adopter signal — not on ambition now.

3. **The architecture keeps the option open by construction, and must continue to.** The
   cell model stays ratatui-compatible (ADR 0003); interop stays a leaf bridge crate that
   never enters core (ADR 0010); the crate layout stays a foundation→family→facade split
   that renames mechanically (ADR 0011); and **no rabbitui public API may encode a naming or
   org assumption.** A change that would make the project hard to reprefix to `ratatui-*` (or
   hard to keep as `rabbitui`) must be justified against this ADR.

4. **The reserved `ratatui-*` framework-layer names are held as protective placeholders**
   (`~/local/ratatui-reservations`), not as an announced product direction. Their existence
   does not constitute a decision to use them; it guarantees the option remains available.

5. **The final call is the author's, at ~0.1.** Because the names are the author's own and
   the org path is pre-cleared, this is a branding/positioning choice to be made — not a race
   to be run or an approval to be sought.

## Consequences

### Positive

- Optionality is preserved at near-zero cost: the highest-leverage adoption move (org names)
  stays available, and so does independence, until there is evidence to choose between them.
- The org-name decision, if made, will be made *after* the correctness moat exists — so the
  name is earned rather than borrowed against an unproven core.
- Technical work is unblocked today; the ratatui-compatibility invariants that keep the door
  open (ADR 0003/0010/0011) are the same ones that give day-one ecosystem interop regardless.
- Reserved names remove any external time pressure — no squatting risk, no competitive clock.

### Negative (honest)

- Deferral forgoes early org-gravity compounding. If the eventual answer is "ship under the
  org," the standalone months are adoption gravity not accrued, and early `rabbitui`-branded
  mindshare, URLs, and docs become sunk cost at rename.
- A late rename has real friction even pre-1.0: crate republication, doc/URL migration,
  redirects, and one round of user confusion. It is cheap relative to a post-1.0 rename but
  not free.
- Carrying two live possibilities imposes a standing constraint — every core change must stay
  naming- and org-agnostic and preserve ratatui-compatibility, a small but permanent tax
  until the decision lands.

### Neutral

- The naming-agnostic invariant coincides with invariants ADRs 0003/0010/0011 already impose
  for interop reasons, so the marginal constraint this ADR adds is small.
- Which specific reserved names would be used (`ratatui-runtime` as the facade vs a different
  mapping onto the crate layout of ADR 0011) is itself deferred to the ~0.1 decision.
- Interop being a soft goal (ADR 0010) is independent of positioning: the bridge ships either
  way; only its status (escape hatch vs first-class org integration) would shift on the call.

## Revisit triggers

- **The ~0.1 milestone arrives.** This is the scheduled decision point: make the
  ship-as/under-`ratatui-*`-or-not call on the evidence then available, and supersede this ADR
  with one recording the outcome and its rationale.
- **A concrete squatting or naming threat to the reserved names emerges** (e.g. a transfer
  request, or the reservation policy changing) — reopen to decide whether to publish real
  crates under the names earlier to defend them.
- **Adoption evidence shifts decisively before ~0.1** — e.g. a standalone framework in the
  wave demonstrably breaks the ~400-star ceiling on its own brand, weakening force (1); or
  ratatui-org governance changes in a way that raises the cost of landing under the org.
- **An architectural pressure threatens the option-keeping invariant** — if a capability
  rabbitui needs (ADR 0003/0013) cannot be delivered while staying mechanically renamable or
  ratatui-compatible, reopen to decide which to sacrifice: the capability or the open option.
- **A forcing function makes the org relationship urgent** — e.g. accessibility work (ADR
  0008 non-goals; ratatui #2190) or an upstream feature-absorption that makes formal
  affiliation materially valuable before ~0.1.
