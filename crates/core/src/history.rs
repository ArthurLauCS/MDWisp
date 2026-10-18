use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{io_err, now_ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RevisionKind {
    Auto,
    Manual,
    Restore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    /// `${sha256[..20]}-${uuid}`, or the bare hash in old indexes; also the blob filename.
    pub id: String,
    pub at: u64,
    pub bytes: usize,
    pub kind: RevisionKind,
    /// Absent in old indexes; filled when listing those snapshots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    revs: Vec<Revision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffKind {
    Same,
    Add,
    Del,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
    /// 1-based.
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffSummary {
    pub added: usize,
    pub removed: usize,
    /// Runs of changes with a little unchanged context around each.
    pub hunks: Vec<Vec<DiffLine>>,
}

const MAX: usize = 200;

fn line_count(text: &str) -> usize {
    if text.is_empty() { 0 } else { text.split('\n').count() }
}

/// Per-document revision history under `<userData>/history/<docId>/`.
///
/// Snapshots live in the app's data rather than beside the document, so a
/// shared folder stays clean and history is never mistaken for content.
/// Consecutive identical content is skipped.
pub struct History {
    root: PathBuf,
    // Autosave and restore can arrive together; one index update must not
    // overwrite the other or race a clear of its blobs.
    lock: Mutex<()>,
}

impl History {
    pub fn new(user_data: &std::path::Path) -> Self {
        Self { root: user_data.join("history"), lock: Mutex::new(()) }
    }

    fn dir(&self, doc_id: &str) -> PathBuf {
        self.root.join(doc_id)
    }

    fn read_index(&self, doc_id: &str) -> Index {
        let index: Index = fs::read_to_string(self.dir(doc_id).join("index.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let mut seen = HashSet::new();
        Index { revs: index.revs.into_iter().filter(|r| seen.insert(r.id.clone())).collect() }
    }

    fn write_index(&self, doc_id: &str, index: &Index) -> Result<(), String> {
        let dir = self.dir(doc_id);
        fs::create_dir_all(&dir).map_err(io_err)?;
        fs::write(dir.join("index.tmp"), serde_json::to_string(index).map_err(|e| e.to_string())?).map_err(io_err)?;
        fs::rename(dir.join("index.tmp"), dir.join("index.json")).map_err(io_err)
    }

    /// Record a snapshot; None when it matches the newest one, or when an
    /// autosave lands within `min_gap_ms` of it.
    pub fn record(&self, doc_id: &str, text: &str, kind: RevisionKind, min_gap_ms: u64) -> Result<Option<Revision>, String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let hash: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>()[..20].to_string();
        let dir = self.dir(doc_id);
        fs::create_dir_all(&dir).map_err(io_err)?;

        let mut index = self.read_index(doc_id);
        if let Some(newest) = index.revs.first() {
            if newest.id.split('-').next() == Some(hash.as_str()) {
                return Ok(None);
            }
            // A burst of typing should leave a few restore points, not one per pause.
            if min_gap_ms > 0 && kind == RevisionKind::Auto && now_ms().saturating_sub(newest.at) < min_gap_ms {
                return Ok(None);
            }
        }

        // The same content can recur after an undo/restore; each occurrence needs its own identity.
        let id = format!("{hash}-{}", uuid::Uuid::new_v4());
        fs::write(dir.join(format!("{id}.md")), text).map_err(io_err)?;
        let rev = Revision { id, at: now_ms(), bytes: text.len(), kind, lines: Some(line_count(text)) };
        index.revs.insert(0, rev.clone());

        // Losing a very old revision beats an unbounded directory.
        for old in index.revs.split_off(MAX.min(index.revs.len())) {
            let _ = fs::remove_file(dir.join(format!("{}.md", old.id)));
        }
        self.write_index(doc_id, &index)?;
        Ok(Some(rev))
    }

    pub fn list(&self, doc_id: &str) -> Vec<Revision> {
        let mut revs = self.read_index(doc_id).revs;
        for rev in revs.iter_mut().filter(|r| r.lines.is_none()) {
            rev.lines = Some(self.read(doc_id, &rev.id).map_or(0, |t| line_count(&t)));
        }
        revs
    }

    /// Revision contents, or None when the blob is gone.
    pub fn read(&self, doc_id: &str, rev_id: &str) -> Option<String> {
        fs::read_to_string(self.dir(doc_id).join(format!("{rev_id}.md"))).ok()
    }

    /// Forget one revision and its blob; false when it wasn't there.
    pub fn forget(&self, doc_id: &str, rev_id: &str) -> Result<bool, String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut index = self.read_index(doc_id);
        let before = index.revs.len();
        index.revs.retain(|r| r.id != rev_id);
        if index.revs.len() == before {
            return Ok(false);
        }
        self.write_index(doc_id, &index)?;
        let _ = fs::remove_file(self.dir(doc_id).join(format!("{rev_id}.md")));
        Ok(true)
    }

    pub fn clear(&self, doc_id: &str) -> Result<(), String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        match fs::remove_dir_all(self.dir(doc_id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    /// Diff of two revisions; an empty id stands for the empty document. None when a blob is gone.
    pub fn diff(&self, doc_id: &str, a: &str, b: &str) -> Option<DiffSummary> {
        let text = |id: &str| if id.is_empty() { Some(String::new()) } else { self.read(doc_id, id) };
        Some(summarise(&diff_lines(&text(a)?, &text(b)?), 3))
    }
}

/// Line-level diff. Hirschberg keeps exact LCS results with linear working
/// memory; trimming unchanged ends limits the expensive part to the edited span.
pub fn diff_lines(before: &str, after: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = before.split('\n').collect();
    let b: Vec<&str> = after.split('\n').collect();
    let mut out = vec![];
    walk(&a, &b, 0, a.len(), 0, b.len(), &mut out);
    out
}

fn line(kind: DiffKind, text: &str, old: Option<usize>, new: Option<usize>) -> DiffLine {
    DiffLine { kind, text: text.to_string(), old_line: old.map(|i| i + 1), new_line: new.map(|j| j + 1) }
}

fn lengths(a: &[&str], b: &[&str], lo: usize, hi: usize, start: usize, end: usize, reverse: bool) -> Vec<u32> {
    let mut row = vec![0u32; end - start + 1];
    for i in 0..hi - lo {
        let mut diagonal = 0;
        for j in 1..row.len() {
            let above = row[j];
            let x = a[if reverse { hi - 1 - i } else { lo + i }];
            let y = b[if reverse { end - j } else { start + j - 1 }];
            row[j] = if x == y { diagonal + 1 } else { row[j].max(row[j - 1]) };
            diagonal = above;
        }
    }
    row
}

fn walk(a: &[&str], b: &[&str], mut lo: usize, mut hi: usize, mut start: usize, mut end: usize, out: &mut Vec<DiffLine>) {
    while lo < hi && start < end && a[lo] == b[start] {
        out.push(line(DiffKind::Same, a[lo], Some(lo), Some(start)));
        lo += 1;
        start += 1;
    }
    let mut suffix = 0;
    while lo < hi && start < end && a[hi - 1] == b[end - 1] {
        hi -= 1;
        end -= 1;
        suffix += 1;
    }
    let common = || {
        let candidates: HashSet<&str> = b[start..end].iter().copied().collect();
        a[lo..hi].iter().any(|l| candidates.contains(l))
    };
    if lo == hi || start == end || !common() {
        for i in lo..hi {
            out.push(line(DiffKind::Del, a[i], Some(i), None));
        }
        for j in start..end {
            out.push(line(DiffKind::Add, b[j], None, Some(j)));
        }
    } else if hi - lo == 1 {
        let matched = (start..b.len()).find(|&j| b[j] == a[lo]);
        for j in start..end {
            out.push(if Some(j) == matched { line(DiffKind::Same, a[lo], Some(lo), Some(j)) } else { line(DiffKind::Add, b[j], None, Some(j)) });
        }
    } else {
        let mid = (lo + hi) / 2;
        let left = lengths(a, b, lo, mid, start, end, false);
        let right = lengths(a, b, mid, hi, start, end, true);
        let n = end - start;
        let mut split = 0;
        for j in 1..=n {
            if left[j] + right[n - j] > left[split] + right[n - split] {
                split = j;
            }
        }
        walk(a, b, lo, mid, start, start + split, out);
        walk(a, b, mid, hi, start + split, end, out);
    }
    for i in 0..suffix {
        out.push(line(DiffKind::Same, a[hi + i], Some(hi + i), Some(end + i)));
    }
}

/// Group a diff into hunks with `context` unchanged lines around each run.
pub fn summarise(diff: &[DiffLine], context: usize) -> DiffSummary {
    let mut spans: Vec<(usize, usize)> = vec![];
    for (at, l) in diff.iter().enumerate() {
        if l.kind == DiffKind::Same {
            continue;
        }
        let from = at.saturating_sub(context);
        let to = (at + context + 1).min(diff.len());
        match spans.last_mut() {
            Some(prev) if from <= prev.1 => prev.1 = to,
            _ => spans.push((from, to)),
        }
    }
    DiffSummary {
        added: diff.iter().filter(|d| d.kind == DiffKind::Add).count(),
        removed: diff.iter().filter(|d| d.kind == DiffKind::Del).count(),
        hunks: spans.into_iter().map(|(f, t)| diff[f..t].to_vec()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RevisionKind::{Auto, Manual, Restore};

    fn svc() -> (tempfile::TempDir, History) {
        let dir = tempfile::tempdir().unwrap();
        let h = History::new(dir.path());
        (dir, h)
    }

    #[test]
    fn reads_legacy_hash_only_snapshots_and_removes_ambiguous_duplicate_identities() {
        let (root, h) = svc();
        let id: String = Sha256::digest(b"legacy").iter().map(|b| format!("{b:02x}")).collect::<String>()[..20].to_string();
        let dir = root.path().join("history").join("doc1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{id}.md")), "legacy").unwrap();
        let rev = serde_json::json!({ "id": id, "at": 1, "bytes": 6, "kind": "manual" });
        fs::write(dir.join("index.json"), serde_json::json!({ "revs": [rev, rev] }).to_string()).unwrap();
        assert_eq!(h.read("doc1", &id).as_deref(), Some("legacy"));
        assert_eq!(h.list("doc1"), [Revision { id: id.clone(), at: 1, bytes: 6, kind: Manual, lines: Some(1) }]);
        assert_eq!(h.record("doc1", "legacy", Manual, 0).unwrap(), None);
    }

    #[test]
    fn records_a_revision_and_reads_it_back() {
        let (_r, h) = svc();
        let rev = h.record("doc1", "hello", Manual, 0).unwrap().unwrap();
        assert_eq!(h.read("doc1", &rev.id).as_deref(), Some("hello"));
        assert_eq!(h.list("doc1").len(), 1);
    }

    #[test]
    fn lists_line_counts_without_rereading_new_snapshot_blobs() {
        let (root, h) = svc();
        h.record("doc1", "", Manual, 0).unwrap();
        h.record("doc1", "one\ntwo\n", Manual, 0).unwrap();
        // Blobs gone: counts can only come from the index.
        for e in fs::read_dir(root.path().join("history").join("doc1")).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "md") {
                fs::remove_file(p).unwrap();
            }
        }
        assert_eq!(h.list("doc1").iter().map(|r| r.lines).collect::<Vec<_>>(), [Some(3), Some(0)]);
    }

    #[test]
    fn keeps_revisions_newest_first() {
        let (_r, h) = svc();
        h.record("doc1", "one", Manual, 0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.record("doc1", "two", Manual, 0).unwrap();
        let list = h.list("doc1");
        assert_eq!((list[0].bytes, list[1].bytes), (3, 3));
        assert!(list[0].at >= list[1].at);
        assert_eq!(h.read("doc1", &list[0].id).as_deref(), Some("two"));
    }

    #[test]
    fn ignores_a_save_that_changes_nothing() {
        let (_r, h) = svc();
        assert!(h.record("doc1", "same", Manual, 0).unwrap().is_some());
        assert!(h.record("doc1", "same", Manual, 0).unwrap().is_none());
        assert_eq!(h.list("doc1").len(), 1);
    }

    #[test]
    fn keeps_revisions_independent_across_documents_with_identical_content() {
        let (_r, h) = svc();
        let a = h.record("doc1", "shared text", Manual, 0).unwrap().unwrap();
        let b = h.record("doc2", "shared text", Manual, 0).unwrap().unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(h.read("doc1", &a.id).as_deref(), Some("shared text"));
        assert_eq!(h.read("doc2", &b.id).as_deref(), Some("shared text"));
    }

    #[test]
    fn keeps_recurring_content_as_separate_restore_points_and_serializes_simultaneous_writes() {
        let (_r, h) = svc();
        let (first, second, repeated) = std::thread::scope(|s| {
            let a = s.spawn(|| h.record("doc1", "A", Manual, 0).unwrap());
            let b = s.spawn(|| h.record("doc1", "B", Manual, 0).unwrap());
            let c = s.spawn(|| h.record("doc1", "A", Restore, 0).unwrap());
            (a.join().unwrap(), b.join().unwrap(), c.join().unwrap())
        });
        // Thread order is not fixed, so "A" twice in a row may collapse into one.
        let made: Vec<_> = [&first, &second, &repeated].into_iter().flatten().map(|r| r.id.clone()).collect();
        let mut listed: Vec<_> = h.list("doc1").into_iter().map(|r| r.id).collect();
        assert_eq!(listed.len(), made.len());
        listed.sort();
        let mut made_sorted = made.clone();
        made_sorted.sort();
        assert_eq!(listed, made_sorted);
        if let (Some(f), Some(r)) = (&first, &repeated) {
            assert_ne!(f.id, r.id);
            h.forget("doc1", &r.id).unwrap();
            assert_eq!(h.read("doc1", &f.id).as_deref(), Some("A"));
        }
        std::thread::scope(|s| {
            s.spawn(|| h.clear("doc1").unwrap());
            s.spawn(|| h.record("doc1", "after clear", Manual, 0).unwrap());
        });
        assert!(h.list("doc1").len() <= 1);
    }

    #[test]
    fn throttles_consecutive_autosave_snapshots() {
        let (_r, h) = svc();
        h.record("doc1", "first", Auto, 10_000).unwrap();
        assert_eq!(h.record("doc1", "second", Auto, 10_000).unwrap(), None);
        assert_eq!(h.list("doc1").len(), 1);
    }

    #[test]
    fn does_not_throttle_a_manual_snapshot() {
        let (_r, h) = svc();
        h.record("doc1", "first", Auto, 10_000).unwrap();
        assert!(h.record("doc1", "second", Manual, 10_000).unwrap().is_some());
        assert_eq!(h.list("doc1").len(), 2);
    }

    #[test]
    fn forgets_one_revision_and_its_blob() {
        let (_r, h) = svc();
        let rev = h.record("doc1", "gone", Manual, 0).unwrap().unwrap();
        assert!(h.forget("doc1", &rev.id).unwrap());
        assert_eq!(h.read("doc1", &rev.id), None);
        assert!(h.list("doc1").is_empty());
    }

    #[test]
    fn reports_when_forgetting_something_that_is_not_there() {
        assert!(!svc().1.forget("doc1", "nope").unwrap());
    }

    #[test]
    fn clears_a_document_entirely() {
        let (_r, h) = svc();
        h.record("doc1", "a", Manual, 0).unwrap();
        h.clear("doc1").unwrap();
        assert!(h.list("doc1").is_empty());
    }

    #[test]
    fn returns_an_empty_list_for_an_unknown_document() {
        let (_r, h) = svc();
        assert!(h.list("never-seen").is_empty());
        assert_eq!(h.read("never-seen", "x"), None);
    }

    #[test]
    fn caps_the_list_and_drops_the_oldest_blobs() {
        let (_r, h) = svc();
        let ids: Vec<_> = (0..205).filter_map(|i| h.record("doc1", &format!("content {i}"), Manual, 0).unwrap()).map(|r| r.id).collect();
        let list = h.list("doc1");
        assert_eq!(list.len(), 200);
        let kept: HashSet<_> = list.iter().map(|r| r.id.as_str()).collect();
        assert!(!kept.contains(ids[0].as_str()));
        assert!(kept.contains(ids[204].as_str()));
        assert_eq!(h.read("doc1", &ids[0]), None);
        assert_eq!(h.read("doc1", &ids[204]).as_deref(), Some("content 204"));
    }

    #[test]
    fn diff_of_two_revisions_treats_an_empty_id_as_an_empty_document() {
        let (_r, h) = svc();
        let rev = h.record("doc1", "a\nb", Manual, 0).unwrap().unwrap();
        assert_eq!(h.diff("doc1", "", &rev.id).unwrap().added, 2);
        assert_eq!(h.diff("doc1", "missing", &rev.id), None);
    }

    fn texts(d: &[DiffLine], kind: DiffKind) -> Vec<&str> {
        d.iter().filter(|l| l.kind == kind).map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn handles_a_small_edit_in_a_very_large_document_without_a_quadratic_table() {
        let mut lines: Vec<String> = (0..20000).map(|i| format!("line {i}")).collect();
        let before = lines.join("\n");
        lines[10000] = "edited".into();
        let result = diff_lines(&before, &lines.join("\n"));
        let changed: Vec<_> = result.into_iter().filter(|l| l.kind != DiffKind::Same).collect();
        assert_eq!(changed, [
            DiffLine { kind: DiffKind::Del, text: "line 10000".into(), old_line: Some(10001), new_line: None },
            DiffLine { kind: DiffKind::Add, text: "edited".into(), old_line: None, new_line: Some(10001) },
        ]);
    }

    #[test]
    fn keeps_exact_minimal_differences_and_offsets_with_repeated_and_reordered_lines() {
        let samples = ["a", "b", "a\na", "a\nb", "b\na", "a\nb\na", "b\na\nb", "", "\na\n", "c\nb\na\nc"];
        for before in samples {
            for after in samples {
                let a: Vec<_> = before.split('\n').collect();
                let b: Vec<_> = after.split('\n').collect();
                let mut lcs = vec![vec![0; b.len() + 1]; a.len() + 1];
                for i in (0..a.len()).rev() {
                    for j in (0..b.len()).rev() {
                        lcs[i][j] = if a[i] == b[j] { 1 + lcs[i + 1][j + 1] } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
                    }
                }
                let result = diff_lines(before, after);
                let side = |skip: DiffKind| result.iter().filter(|l| l.kind != skip).map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
                assert_eq!(side(DiffKind::Add), before);
                assert_eq!(side(DiffKind::Del), after);
                assert_eq!(texts(&result, DiffKind::Same).len(), lcs[0][0]);
                for l in &result {
                    if let Some(o) = l.old_line {
                        assert_eq!(a[o - 1], l.text);
                    }
                    if let Some(n) = l.new_line {
                        assert_eq!(b[n - 1], l.text);
                    }
                }
            }
        }
    }

    #[test]
    fn reports_no_changes_for_identical_text() {
        assert!(diff_lines("a\nb", "a\nb").iter().all(|l| l.kind == DiffKind::Same));
    }

    #[test]
    fn marks_an_appended_line_as_an_addition() {
        assert_eq!(texts(&diff_lines("a", "a\nb"), DiffKind::Add), ["b"]);
    }

    #[test]
    fn marks_a_removed_line_as_a_deletion() {
        assert_eq!(texts(&diff_lines("a\nb", "a"), DiffKind::Del), ["b"]);
    }

    #[test]
    fn tracks_line_numbers_on_both_sides() {
        let d = diff_lines("a\nb\nc", "a\nx\nc");
        let del = d.iter().find(|l| l.kind == DiffKind::Del).unwrap();
        let add = d.iter().find(|l| l.kind == DiffKind::Add).unwrap();
        assert_eq!((del.text.as_str(), del.old_line, del.new_line), ("b", Some(2), None));
        assert_eq!((add.text.as_str(), add.old_line, add.new_line), ("x", None, Some(2)));
    }

    #[test]
    fn survives_a_change_at_the_very_start() {
        assert_eq!(texts(&diff_lines("old\nkeep", "new\nkeep"), DiffKind::Same), ["keep"]);
    }

    #[test]
    fn handles_an_empty_document() {
        assert!(diff_lines("", "new").iter().any(|l| l.kind == DiffKind::Add));
    }

    #[test]
    fn reconstructs_the_new_text_from_the_diff() {
        let after = "a\nX\nc\nd\ne";
        let rebuilt: Vec<_> = diff_lines("a\nb\nc\nd", after).into_iter().filter(|l| l.kind != DiffKind::Del).map(|l| l.text).collect();
        assert_eq!(rebuilt.join("\n"), after);
    }

    #[test]
    fn keeps_context_in_order_without_duplicating_or_dropping_distant_changes() {
        let before = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let after = before.replacen("line 1\n", "changed 1\n", 1).replacen("line 25\n", "changed 25\n", 1);
        let result = summarise(&diff_lines(&before, &after), 2);
        assert_eq!(result.hunks.len(), 2);
        assert_eq!(texts(&result.hunks[0], DiffKind::Same), ["line 0", "line 2", "line 3"]);
        assert_eq!(texts(&result.hunks.concat(), DiffKind::Add), ["changed 1", "changed 25"]);
    }

    #[test]
    fn counts_additions_and_deletions() {
        let s = summarise(&diff_lines("a\nb", "a\nc\nd"), 3);
        assert_eq!((s.added, s.removed), (2, 1));
    }

    #[test]
    fn reports_no_hunks_for_identical_text() {
        assert!(summarise(&diff_lines("a\nb", "a\nb"), 3).hunks.is_empty());
    }

    #[test]
    fn keeps_a_changed_line_inside_a_hunk() {
        let flat = summarise(&diff_lines("a\nb\nc", "a\nX\nc"), 3).hunks.concat();
        assert!(flat.iter().any(|l| l.kind == DiffKind::Del && l.text == "b"));
        assert!(flat.iter().any(|l| l.kind == DiffKind::Add && l.text == "X"));
    }
}
