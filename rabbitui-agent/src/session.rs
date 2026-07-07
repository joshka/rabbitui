//! Transcript persistence and resume.
//!
//! A session is a JSONL file: a first `meta` line (model, created-at, title) then
//! one API-shaped [`ChatMessage`] per line. Resume is therefore just "deserialize
//! the messages and keep going" — the same history the backend replays. Files live
//! under `${XDG_DATA_HOME:-~/.local/share}/rabbitui-agent/sessions/<created>.jsonl`.
//!
//! Slice 1 persists text-only messages; slices 2 and 4 widen [`ChatMessage`] to
//! content blocks, and this format widens with it (the meta line is versioned so
//! an older file is recognizable).

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backend::ChatMessage;

/// The first line of a session file: everything but the conversation itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    /// The persisted-format version, so an older file is recognizable.
    pub version: u32,
    /// The model the session ran against.
    pub model: String,
    /// A wall-clock stamp (seconds since the Unix epoch) naming the file.
    pub created: u64,
    /// A short title, derived from the first user prompt.
    pub title: String,
}

/// The current session-file format version.
const FORMAT_VERSION: u32 = 1;

/// One line of a session file: either the metadata header or a message.
///
/// Untagged so the file stays a clean stream of `ChatMessage`s after the header —
/// a header has a `version` field a message never does, which disambiguates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Line {
    /// The header line (first line only).
    Meta(SessionMeta),
    /// A conversation message.
    Message(ChatMessage),
}

/// An append-only session log on disk.
#[derive(Debug)]
pub struct Session {
    /// The file this session writes to.
    path: PathBuf,
    /// The metadata header (written lazily on the first append).
    meta: SessionMeta,
    /// Whether the header has been written yet.
    header_written: bool,
}

impl Session {
    /// Opens a fresh session for `model`, stamped `created` (seconds since epoch —
    /// passed in rather than read from the clock so callers and tests stay
    /// deterministic). The file is created on the first [`append`](Self::append).
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the sessions directory cannot be created.
    pub fn create(model: impl Into<String>, created: u64) -> std::io::Result<Self> {
        let dir = sessions_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{created}.jsonl"));
        Ok(Self {
            path,
            meta: SessionMeta {
                version: FORMAT_VERSION,
                model: model.into(),
                created,
                title: String::new(),
            },
            header_written: false,
        })
    }

    /// The file this session writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sets the session title (from the first user prompt), if not already set.
    pub fn set_title_if_empty(&mut self, title: impl Into<String>) {
        if self.meta.title.is_empty() {
            self.meta.title = title.into();
        }
    }

    /// Appends one message to the file, writing the header first if needed.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file cannot be opened or written.
    pub fn append(&mut self, message: &ChatMessage) -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        if !self.header_written {
            writeln!(file, "{}", to_line(&Line::Meta(self.meta.clone()))?)?;
            self.header_written = true;
        }
        writeln!(file, "{}", to_line(&Line::Message(message.clone()))?)?;
        Ok(())
    }

    /// Loads a session file, returning its metadata and message history.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file cannot be read or a line cannot be parsed.
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<(SessionMeta, Vec<ChatMessage>)> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)?;
        let mut meta = None;
        let mut messages = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            match serde_json::from_str::<Line>(line).map_err(invalid_data)? {
                Line::Meta(header) => meta = Some(header),
                Line::Message(message) => messages.push(message),
            }
        }
        let meta = meta.ok_or_else(|| invalid_data("session file has no metadata header"))?;
        Ok((meta, messages))
    }

    /// Reopens an existing session file for appending (resume), preserving its
    /// metadata so continued messages join the same file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file cannot be read or parsed.
    pub fn resume(path: impl AsRef<Path>) -> std::io::Result<(Self, Vec<ChatMessage>)> {
        let path = path.as_ref().to_path_buf();
        let (meta, messages) = Self::load(&path)?;
        Ok((
            Self {
                path,
                meta,
                header_written: true,
            },
            messages,
        ))
    }

    /// The most recent session file in the sessions directory, if any.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the directory cannot be read.
    pub fn latest() -> std::io::Result<Option<PathBuf>> {
        let dir = sessions_dir()?;
        if !dir.exists() {
            return Ok(None);
        }
        let latest = std::fs::read_dir(&dir)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .max();
        Ok(latest)
    }
}

/// Serializes one session line to a single-line JSON string.
fn to_line(line: &Line) -> std::io::Result<String> {
    serde_json::to_string(line).map_err(invalid_data)
}

/// Wraps a display-able error as an `InvalidData` I/O error.
fn invalid_data(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
}

/// The directory session files live in.
fn sessions_dir() -> std::io::Result<PathBuf> {
    let base = if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(data)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "neither XDG_DATA_HOME nor HOME is set; cannot locate a sessions directory",
        ));
    };
    Ok(base.join("rabbitui-agent/sessions"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// A unique temp path, so this test targets an explicit file and never
    /// touches the real sessions directory (or races other tests).
    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rabbitui-agent-{}-{n}.jsonl", std::process::id()))
    }

    /// Writes a session file by hand from public types. `Line` is untagged, so a
    /// bare `SessionMeta`/`ChatMessage` serializes identically to what `append`
    /// writes — this exercises the real `load`/`resume` path.
    fn write_fixture(path: &Path, messages: &[ChatMessage]) {
        let meta = SessionMeta {
            version: FORMAT_VERSION,
            model: "test-model".to_string(),
            created: 42,
            title: "greeting".to_string(),
        };
        let mut text = serde_json::to_string(&meta).unwrap();
        text.push('\n');
        for message in messages {
            text.push_str(&serde_json::to_string(message).unwrap());
            text.push('\n');
        }
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn load_reads_meta_and_messages() {
        let path = temp_path();
        write_fixture(
            &path,
            &[ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        );

        let (meta, messages) = Session::load(&path).unwrap();
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.title, "greeting");
        assert_eq!(messages, vec![
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
        ]);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn resume_then_append_extends_the_same_file() {
        let path = temp_path();
        write_fixture(&path, &[ChatMessage::user("hi")]);

        let (mut session, resumed) = Session::resume(&path).unwrap();
        assert_eq!(resumed, vec![ChatMessage::user("hi")]);

        session.append(&ChatMessage::assistant("hello")).unwrap();
        session.append(&ChatMessage::user("again")).unwrap();

        let (meta, messages) = Session::load(&path).unwrap();
        assert_eq!(meta.title, "greeting", "resume preserves the header");
        assert_eq!(messages, vec![
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello"),
            ChatMessage::user("again"),
        ]);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_rejects_a_file_with_no_header() {
        let path = temp_path();
        std::fs::write(&path, "{\"role\":\"user\",\"content\":\"orphan\"}\n").unwrap();
        assert!(Session::load(&path).is_err(), "a headerless file is invalid");
        std::fs::remove_file(&path).ok();
    }
}
