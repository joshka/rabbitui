//! Golden-snapshot assertions with an update flag.
//!
//! Per `docs/adr/0009-testing.md`, buffer snapshots are the base of the test
//! pyramid, compared against stored files and regenerated behind an update flag
//! (`teatest`/`pytest-textual-snapshot` `-update` semantics). This module is the
//! dependency-free file half; [`crate::TestApp::buffer_text`] produces the text.
//!
//! A snapshot lives at `<manifest_dir>/tests/snapshots/<name>.txt`. The manifest
//! directory is the *calling* crate's, so each crate keeps its own snapshots
//! next to its tests — pass it explicitly to [`assert_snapshot`], or let the
//! [`assert_snapshot!`](macro@crate::assert_snapshot) macro capture
//! `env!("CARGO_MANIFEST_DIR")` for you.
//!
//! When the environment variable `UPDATE_SNAPSHOTS=1` is set, a mismatched or
//! missing snapshot is rewritten from `actual` and the assertion passes, so
//! accepting a reviewed change is a single run. Without it, a missing or
//! differing snapshot panics with a readable diff.
//!
//! # Examples
//!
//! In a test in some crate, capturing that crate's manifest dir:
//!
//! ```no_run
//! use rabbitui_testing::assert_snapshot;
//!
//! # fn demo() {
//! let rendered = "line one\nline two".to_string();
//! assert_snapshot!("my_view", rendered);
//! # }
//! ```

use std::path::{Path, PathBuf};

/// Asserts `actual` matches the snapshot named `name` under `manifest_dir`.
///
/// The snapshot path is `<manifest_dir>/tests/snapshots/<name>.txt`. Prefer the
/// [`assert_snapshot!`](macro@crate::assert_snapshot) macro, which fills `manifest_dir` from the calling
/// crate's `CARGO_MANIFEST_DIR`; call this directly only when the directory is
/// computed some other way.
///
/// A single trailing newline in the stored file is ignored, so a snapshot reads
/// naturally in an editor whether or not `actual` ended in one.
///
/// # Behavior
///
/// - `UPDATE_SNAPSHOTS=1`: the snapshot file (and its parent directories) is
///   written from `actual` and the assertion passes.
/// - otherwise: a missing or differing snapshot panics; the message names the
///   file, shows the diff, and points at the update flag.
///
/// # Panics
///
/// Panics if the snapshot is missing or differs and `UPDATE_SNAPSHOTS` is not
/// set, or if writing the snapshot under `UPDATE_SNAPSHOTS=1` fails.
///
/// # Examples
///
/// ```no_run
/// use rabbitui_testing::snapshot::assert_snapshot;
///
/// let actual = "hello".to_string();
/// assert_snapshot("greeting", actual, env!("CARGO_MANIFEST_DIR"));
/// ```
pub fn assert_snapshot(name: &str, actual: impl AsRef<str>, manifest_dir: impl AsRef<Path>) {
    let actual = actual.as_ref();
    let path = snapshot_path(manifest_dir.as_ref(), name);

    if update_requested() {
        write_snapshot(&path, actual);
        return;
    }

    let Some(expected) = read_snapshot(&path) else {
        panic!(
            "snapshot {name:?} is missing at {}\n\
             run with UPDATE_SNAPSHOTS=1 to create it. actual output was:\n{actual}",
            path.display(),
        );
    };

    assert!(
        expected == actual,
        "snapshot {name:?} did not match {}\n\
         --- expected ---\n{expected}\n--- actual ---\n{actual}\n\
         --- end ---\nrun with UPDATE_SNAPSHOTS=1 to accept the new output.",
        path.display(),
    );
}

/// True when `UPDATE_SNAPSHOTS` is set to `1`.
fn update_requested() -> bool {
    std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1")
}

/// The snapshot file path for `name` under `manifest_dir`.
fn snapshot_path(manifest_dir: &Path, name: &str) -> PathBuf {
    let mut path = manifest_dir.join("tests").join("snapshots").join(name);
    path.set_extension("txt");
    path
}

/// Reads a snapshot, normalizing a single trailing newline away, or `None` if
/// the file does not exist.
fn read_snapshot(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    Some(
        contents
            .strip_suffix('\n')
            .map_or(contents.clone(), str::to_string),
    )
}

/// Writes `actual` to `path` as the accepted snapshot, creating parent
/// directories and appending one trailing newline for editor-friendliness.
fn write_snapshot(path: &Path, actual: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("creating {}: {error}", parent.display()));
    }
    std::fs::write(path, format!("{actual}\n"))
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
}

/// Asserts `$actual` matches the snapshot `$name`, using the *calling* crate's
/// `CARGO_MANIFEST_DIR` so snapshots live beside that crate's tests.
///
/// A thin wrapper over [`assert_snapshot`] that captures the manifest directory
/// at the call site; see that function for the file layout and update-flag
/// semantics.
///
/// # Examples
///
/// ```no_run
/// use rabbitui_testing::assert_snapshot;
///
/// # fn demo() {
/// let rendered = "a\nb".to_string();
/// assert_snapshot!("two_lines", rendered);
/// # }
/// ```
#[macro_export]
macro_rules! assert_snapshot {
    ($name:expr, $actual:expr $(,)?) => {
        $crate::snapshot::assert_snapshot($name, $actual, env!("CARGO_MANIFEST_DIR"))
    };
}

#[cfg(test)]
mod tests {
    use super::{read_snapshot, snapshot_path};
    use std::path::Path;

    #[test]
    fn snapshot_path_is_under_tests_snapshots() {
        let path = snapshot_path(Path::new("/crate"), "counter");
        assert!(
            path.ends_with("tests/snapshots/counter.txt"),
            "{}",
            path.display()
        );
    }

    #[test]
    fn reading_a_missing_snapshot_is_none() {
        let path = snapshot_path(Path::new("/definitely/not/here"), "nope");
        assert!(read_snapshot(&path).is_none());
    }

    #[test]
    fn write_then_read_round_trips_without_trailing_newline() {
        let dir = std::env::temp_dir().join(format!("rabbitui-snap-{}", std::process::id()));
        let path = snapshot_path(&dir, "round_trip");
        super::write_snapshot(&path, "a\nb");
        // The trailing newline the writer adds is stripped back off on read.
        assert_eq!(read_snapshot(&path).as_deref(), Some("a\nb"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
