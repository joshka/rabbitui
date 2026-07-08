//! Two read-only, cwd-confined tools: `read_file` and `list_dir`.
//!
//! This slice exists to exercise modal-over-transcript routing and live
//! tool-call status cells — not to build an agent harness — so the tools are
//! deliberately safe. Both **confine to the cwd subtree**: the requested path
//! and the cwd are each canonicalized, and the call is rejected unless the
//! canonical target is inside the canonical cwd. Canonicalization resolves
//! `..` and symlinks, so neither `../escape` nor a symlink pointing outside the
//! tree can read a file we shouldn't. Errors are returned as `Err(String)` and
//! surfaced to the model as an `is_error` tool result — we never read outside.
//!
//! Two things are exposed: [`declarations`] — the JSON-schema `tools` array for
//! the request body — and [`execute`], a pure `(name, input) -> Result` the app
//! calls once the user allows a tool call.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The cap on a single `read_file` result; longer files are truncated with a
/// note so a huge file can't blow up the transcript or the next request.
const READ_CAP: usize = 64 * 1024;

/// The JSON-schema declarations for the request's `tools` array.
///
/// The shape is the Anthropic wire's: each entry is
/// `{"name", "description", "input_schema": {"type": "object", ...}}`.
#[must_use]
pub fn declarations() -> Value {
    json!([
        {
            "name": "read_file",
            "description": "Read a UTF-8 text file within the current working directory. \
                            The path must resolve inside the working directory subtree.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file, relative to the working directory."
                    }
                },
                "required": ["path"]
            }
        },
        {
            "name": "list_dir",
            "description": "List the entries of a directory within the current working \
                            directory. The path must resolve inside the working directory subtree.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory, relative to the working directory. \
                                        Defaults to \".\" (the working directory itself)."
                    }
                },
                "required": []
            }
        }
    ])
}

/// Runs tool `name` with JSON `input`, against the process cwd.
///
/// `Ok` carries the result text (a tool result), `Err` carries an error message
/// (an `is_error` tool result). An unknown tool name is an `Err`.
///
/// # Errors
///
/// Returns an error string for an unknown tool, a bad/missing argument, a path
/// that escapes the cwd subtree, or an I/O failure.
pub fn execute(name: &str, input: &Value) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("cannot resolve cwd: {error}"))?;
    execute_in(&cwd, name, input)
}

/// Runs a tool against an explicit `root` (the cwd in production; a temp dir in
/// tests). Split out from [`execute`] so confinement is testable without
/// mutating the process cwd.
pub fn execute_in(root: &Path, name: &str, input: &Value) -> Result<String, String> {
    match name {
        "read_file" => read_file(root, input),
        "list_dir" => list_dir(root, input),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// A one-line summary of a tool call for the confirmation modal and the Tool
/// cell header (e.g. `read_file(src/lib.rs)`).
#[must_use]
pub fn summarize(name: &str, input: &Value) -> String {
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    format!("{name}({path})")
}

/// Reads the `path` argument's file, capped at [`READ_CAP`] bytes.
fn read_file(root: &Path, input: &Value) -> Result<String, String> {
    let requested = require_str(input, "path")?;
    let target = confined(root, requested)?;
    let bytes = std::fs::read(&target).map_err(|error| format!("cannot read file: {error}"))?;
    let text = String::from_utf8_lossy(&bytes);
    if text.len() > READ_CAP {
        let mut end = READ_CAP;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Ok(format!(
            "{}\n\n[truncated: {} of {} bytes shown]",
            &text[..end],
            end,
            text.len()
        ))
    } else {
        Ok(text.into_owned())
    }
}

/// Lists the `path` argument's directory (defaulting to `.`), sorted, each entry
/// tagged as a dir or a file.
fn list_dir(root: &Path, input: &Value) -> Result<String, String> {
    let requested = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let target = confined(root, requested)?;
    let mut entries: Vec<(String, bool)> = std::fs::read_dir(&target)
        .map_err(|error| format!("cannot list directory: {error}"))?
        .filter_map(std::result::Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            (name, is_dir)
        })
        .collect();
    entries.sort();
    if entries.is_empty() {
        return Ok("(empty directory)".to_string());
    }
    let listing = entries
        .into_iter()
        .map(|(name, is_dir)| {
            if is_dir {
                format!("{name}/  (dir)")
            } else {
                format!("{name}  (file)")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(listing)
}

/// Resolves `requested` (relative to `root`) and rejects it unless the canonical
/// target is inside the canonical `root`. Canonicalization resolves `..` and
/// symlinks, so this catches traversal and symlink escapes alike.
fn confined(root: &Path, requested: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve working directory: {error}"))?;
    let joined = root.join(requested);
    let target = joined
        .canonicalize()
        .map_err(|error| format!("cannot resolve path {requested:?}: {error}"))?;
    if target.starts_with(&root) {
        Ok(target)
    } else {
        Err(format!(
            "path {requested:?} escapes the working directory and was refused"
        ))
    }
}

/// Extracts a required string argument.
fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string argument {key:?}"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// A unique temp directory seeded with a small tree, for confinement tests.
    fn temp_root() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rabbitui-tools-{}-{n}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("hello.txt"), "hi there\n").unwrap();
        std::fs::write(dir.join("sub/nested.txt"), "nested\n").unwrap();
        // A sibling *outside* the root, the target of an escape attempt.
        let secret = dir.parent().unwrap().join(format!(
            "rabbitui-tools-secret-{}-{n}.txt",
            std::process::id()
        ));
        std::fs::write(&secret, "TOP SECRET\n").unwrap();
        dir
    }

    #[test]
    fn read_file_returns_contents() {
        let root = temp_root();
        let out = execute_in(&root, "read_file", &json!({"path": "hello.txt"})).unwrap();
        assert_eq!(out, "hi there\n");
    }

    #[test]
    fn list_dir_is_sorted_with_kinds() {
        let root = temp_root();
        let out = execute_in(&root, "list_dir", &json!({"path": "."})).unwrap();
        assert_eq!(out, "hello.txt  (file)\nsub/  (dir)");
    }

    #[test]
    fn list_dir_defaults_to_root() {
        let root = temp_root();
        let with = execute_in(&root, "list_dir", &json!({"path": "."})).unwrap();
        let without = execute_in(&root, "list_dir", &json!({})).unwrap();
        assert_eq!(with, without);
    }

    #[test]
    fn dot_dot_traversal_is_refused() {
        let root = temp_root();
        let n = root.file_name().unwrap().to_string_lossy();
        // Reconstruct the sibling secret's relative path via `..`.
        let suffix = n.trim_start_matches("rabbitui-tools-");
        let escape = format!("../rabbitui-tools-secret-{suffix}.txt");
        let err = execute_in(&root, "read_file", &json!({"path": escape})).unwrap_err();
        assert!(
            err.contains("escapes") || err.contains("cannot resolve"),
            "traversal must be refused, got: {err}"
        );
    }

    #[test]
    fn absolute_path_outside_root_is_refused() {
        let root = temp_root();
        let err = execute_in(&root, "read_file", &json!({"path": "/etc/hosts"})).unwrap_err();
        assert!(
            err.contains("escapes"),
            "absolute escape must be refused: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_refused() {
        let root = temp_root();
        // A file outside the root, and a symlink inside the root pointing at it.
        let outside = root.join(format!(
            "../rabbitui-tools-link-target-{}.txt",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&outside, "OUTSIDE\n").unwrap();
        let link = root.join("escape_link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let err = execute_in(&root, "read_file", &json!({"path": "escape_link"})).unwrap_err();
        assert!(
            err.contains("escapes"),
            "a symlink pointing outside the root must be refused: {err}"
        );
    }

    #[test]
    fn unknown_tool_is_an_error() {
        let root = temp_root();
        let err = execute_in(&root, "delete_everything", &json!({})).unwrap_err();
        assert!(err.contains("unknown tool"), "got: {err}");
    }

    #[test]
    fn read_file_missing_path_argument_errors() {
        let root = temp_root();
        let err = execute_in(&root, "read_file", &json!({})).unwrap_err();
        assert!(err.contains("missing required"), "got: {err}");
    }

    #[test]
    fn declarations_shape_matches_the_wire() {
        let decls = declarations();
        let arr = decls.as_array().expect("declarations is an array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "read_file");
        assert_eq!(arr[0]["input_schema"]["type"], "object");
        assert_eq!(arr[0]["input_schema"]["required"][0], "path");
        assert_eq!(arr[1]["name"], "list_dir");
    }
}
