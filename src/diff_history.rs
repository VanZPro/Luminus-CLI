//! File edit history with undo/redo stacks and per-file revert.
//!
//! Each edit records the path, before-content, after-content, and a timestamp.
//! `undo` writes the before-content back to disk and moves the record to the
//! redo stack. `redo` writes the after-content back to disk and moves it back to
//! the undo stack. `revert_file` finds the **earliest** recorded edit for a path
//! (its initial before state) and writes that content back to disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A single file edit record capturing before/after content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEditRecord {
    pub path: PathBuf,
    pub before_content: String,
    pub after_content: String,
    pub timestamp: u64,
}

impl FileEditRecord {
    /// Count of added lines (lines in `after` but not in `before`).
    pub fn added_lines(&self) -> usize {
        let before: std::collections::HashSet<&str> = self.before_content.lines().collect();
        self.after_content
            .lines()
            .filter(|line| !before.contains(line))
            .count()
    }

    /// Count of removed lines (lines in `before` but not in `after`).
    pub fn removed_lines(&self) -> usize {
        let after: std::collections::HashSet<&str> = self.after_content.lines().collect();
        self.before_content
            .lines()
            .filter(|line| !after.contains(line))
            .count()
    }
}

/// Undo/redo history for file edits within a session.
#[derive(Debug, Default)]
pub struct DiffHistory {
    pub undo_stack: Vec<FileEditRecord>,
    pub redo_stack: Vec<FileEditRecord>,
}

impl DiffHistory {
    /// Record a file edit. Call this **after** the edit has been applied to
    /// disk, passing the content the file had before the edit.
    pub fn record_edit(
        &mut self,
        path: impl Into<PathBuf>,
        before: impl Into<String>,
        after: impl Into<String>,
    ) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.undo_stack.push(FileEditRecord {
            path: path.into(),
            before_content: before.into(),
            after_content: after.into(),
            timestamp,
        });
        // Any new edit invalidates the redo stack.
        self.redo_stack.clear();
    }

    /// Undo the most recent edit: write `before_content` back to disk and push
    /// the record onto the redo stack.
    pub fn undo(&mut self) -> Option<FileEditRecord> {
        let record = self.undo_stack.pop()?;
        let _ = fs::write(&record.path, &record.before_content);
        self.redo_stack.push(record.clone());
        Some(record)
    }

    /// Redo the most recently undone edit: write `after_content` back to disk
    /// and push the record back onto the undo stack.
    pub fn redo(&mut self) -> Option<FileEditRecord> {
        let record = self.redo_stack.pop()?;
        let _ = fs::write(&record.path, &record.after_content);
        self.undo_stack.push(record.clone());
        Some(record)
    }

    /// Revert a specific file to its **initial** before state — the earliest
    /// recorded edit for that path. Removes all undo records for that path and
    /// clears redo.
    pub fn revert_file(&mut self, target: &Path) -> Option<FileEditRecord> {
        // Find the earliest record for this path.
        let initial = self.undo_stack.iter().find(|r| r.path == target)?.clone();

        let _ = fs::write(&initial.path, &initial.before_content);

        // Remove all records for this path from both stacks.
        self.undo_stack.retain(|r| r.path != target);
        self.redo_stack.retain(|r| r.path != target);

        Some(initial)
    }

    /// List all modified paths with their cumulative added/removed line counts.
    ///
    /// Returns `(path, added_lines, removed_lines)`.
    pub fn changes(&self) -> Vec<(PathBuf, usize, usize)> {
        // Collect unique paths in insertion order.
        let mut ordered: Vec<PathBuf> = Vec::new();
        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for record in &self.undo_stack {
            if seen.insert(record.path.clone()) {
                ordered.push(record.path.clone());
            }
        }

        ordered
            .into_iter()
            .map(|path| {
                let (added, removed) = self
                    .undo_stack
                    .iter()
                    .filter(|r| r.path == path)
                    .fold((0usize, 0usize), |(a, r), record| {
                        (a + record.added_lines(), r + record.removed_lines())
                    });
                (path, added, removed)
            })
            .collect()
    }

    /// Generate a unified diff string for all current undo records.
    pub fn unified_diff(&self) -> String {
        let mut output = String::new();
        for record in &self.undo_stack {
            let diff =
                unified_diff_for(&record.path, &record.before_content, &record.after_content);
            output.push_str(&diff);
        }
        output
    }

    /// Number of edits on the undo stack.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Number of edits on the redo stack.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Whether there are any changes recorded.
    pub fn is_empty(&self) -> bool {
        self.undo_stack.is_empty() && self.redo_stack.is_empty()
    }
}

/// Produce a minimal unified diff between two string contents for a given path.
pub fn unified_diff_for(path: &Path, before: &str, after: &str) -> String {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    let display = path.display().to_string();
    let mut output = format!("--- a/{display}\n+++ b/{display}\n");

    // Simple line-level diff: find common prefix and suffix.
    let prefix_len = before_lines
        .iter()
        .zip(after_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let suffix_len = before_lines[prefix_len..]
        .iter()
        .rev()
        .zip(after_lines[prefix_len..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let before_mid = &before_lines[prefix_len..before_lines.len().saturating_sub(suffix_len)];
    let after_mid = &after_lines[prefix_len..after_lines.len().saturating_sub(suffix_len)];

    let before_start = prefix_len + 1; // 1-indexed
    let after_start = prefix_len + 1;
    let hunk_len = before_mid.len().max(after_mid.len());

    if hunk_len == 0 && before_mid.is_empty() && after_mid.is_empty() {
        return output; // no changes
    }

    output.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        before_start,
        before_mid.len(),
        after_start,
        after_mid.len()
    ));

    for line in before_mid {
        output.push('-');
        output.push_str(line);
        output.push('\n');
    }
    for line in after_mid {
        output.push('+');
        output.push_str(line);
        output.push('\n');
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "luminus-diff-history-{}-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            tag
        ))
    }

    #[test]
    fn record_and_undo_restores_before() {
        let path = temp_path("undo");
        fs::write(&path, "line1\nMODIFIED\n").unwrap();

        let mut history = DiffHistory::default();
        history.record_edit(&path, "line1\nline2\n", "line1\nMODIFIED\n");

        assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nMODIFIED\n");

        let record = history.undo().expect("should undo");
        assert_eq!(record.path, path);
        assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2\n");
        assert_eq!(history.undo_count(), 0);
        assert_eq!(history.redo_count(), 1);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn redo_reapplies_after() {
        let path = temp_path("redo");
        fs::write(&path, "original\n").unwrap();

        let mut history = DiffHistory::default();
        history.record_edit(&path, "original\n", "changed\n");
        history.undo();

        assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");

        let _record = history.redo().expect("should redo");
        assert_eq!(fs::read_to_string(&path).unwrap(), "changed\n");
        assert_eq!(history.undo_count(), 1);
        assert_eq!(history.redo_count(), 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn new_edit_clears_redo() {
        let path = temp_path("clear-redo");
        fs::write(&path, "a\n").unwrap();

        let mut history = DiffHistory::default();
        history.record_edit(&path, "a\n", "b\n");
        history.undo();
        assert_eq!(history.redo_count(), 1);

        history.record_edit(&path, "a\n", "c\n");
        assert_eq!(history.redo_count(), 0);
        assert_eq!(history.undo_count(), 1);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn revert_file_restores_initial_state() {
        let path = temp_path("revert");
        fs::write(&path, "second_edit\n").unwrap();

        let mut history = DiffHistory::default();
        history.record_edit(&path, "initial\n", "first_edit\n");
        history.record_edit(&path, "first_edit\n", "second_edit\n");

        assert_eq!(fs::read_to_string(&path).unwrap(), "second_edit\n");

        let record = history.revert_file(&path).expect("should revert");
        assert_eq!(record.before_content, "initial\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "initial\n");
        assert_eq!(history.undo_count(), 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn changes_reports_line_counts() {
        let path = temp_path("changes");
        let mut history = DiffHistory::default();
        history.record_edit(&path, "line1\nline2\n", "line1\nREPLACED\n");

        let changes = history.changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, path);
        assert!(changes[0].1 > 0); // added
        assert!(changes[0].2 > 0); // removed
    }

    #[test]
    fn unified_diff_contains_markers() {
        let path = Path::new("test.txt");
        let diff = unified_diff_for(path, "hello\nworld\n", "hello\nWORLD\n");
        assert!(diff.contains("--- a/"));
        assert!(diff.contains("+++ b/"));
        assert!(diff.contains("@@"));
        assert!(diff.contains("-world"));
        assert!(diff.contains("+WORLD"));
    }

    #[test]
    fn undo_on_empty_returns_none() {
        let mut history = DiffHistory::default();
        assert!(history.undo().is_none());
        assert!(history.redo().is_none());
    }

    #[test]
    fn revert_nonexistent_file_returns_none() {
        let mut history = DiffHistory::default();
        assert!(history.revert_file(Path::new("nonexistent.txt")).is_none());
    }

    #[test]
    fn multiple_files_changes() {
        let path_a = temp_path("multi-a");
        let path_b = temp_path("multi-b");
        let mut history = DiffHistory::default();
        history.record_edit(&path_a, "a1\n", "a2\n");
        history.record_edit(&path_b, "b1\n", "b2\n");

        let changes = history.changes();
        assert_eq!(changes.len(), 2);
    }

    #[test]
    fn is_empty_and_counts() {
        let mut history = DiffHistory::default();
        assert!(history.is_empty());
        assert_eq!(history.undo_count(), 0);
        assert_eq!(history.redo_count(), 0);

        let path = temp_path("counts");
        history.record_edit(&path, "x\n", "y\n");
        assert!(!history.is_empty());
        assert_eq!(history.undo_count(), 1);
    }
}
