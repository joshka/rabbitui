//! A terminal chat/agent client built on rabbitui.
//!
//! This crate is the framework's flagship and permanent acceptance test (Arc 3;
//! see `docs/plans/arc3-agent-client.md`). It is Anthropic-API-backed, but every
//! network-shaped concern sits behind the [`Backend`](backend::Backend) trait so
//! the whole UI is testable offline against recorded fixtures.
//!
//! Slice 1 lands the crate skeleton, the backend contract, a replay backend,
//! transcript persistence, and the ported chat UI — no network code. The reducer
//! ([`app::apply_message`], [`app::on_submit`]) is pure so [`Agent`](app::Agent)'s `update` side
//! effects (commits, spawns, persistence) stay thin and the whole flow is
//! testable through `rabbitui_testing::TestApp`.

pub mod app;
pub mod backend;
pub mod demo;
pub mod keymap;
pub mod markdown;
pub mod session;
pub mod tools;
pub mod transcript;
