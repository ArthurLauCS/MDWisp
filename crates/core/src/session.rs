use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::{io_err, now_ms};

/// Persisted as `session.json`. Cursor offsets are UTF-8 byte offsets; files
/// written by the Electron version hold UTF-16 offsets, which only shifts a
/// restored caret on lines with non-ASCII text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionState {
    pub workspace_root: Option<PathBuf>,
    pub active_doc: Option<PathBuf>,
    pub open_docs: Vec<PathBuf>,
    pub cursors: HashMap<PathBuf, usize>,
    pub scrolls: HashMap<PathBuf, f64>,
    pub updated_at: u64,
    /// True on the very first launch after install.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_run: Option<bool>,
}

/// recent.json is shared by every window; its read/modify/write must not interleave.
static RECENT: Mutex<()> = Mutex::new(());

/// One window's state between launches.
///
/// Written on change and read once at boot. Every path is validated on read:
/// a document that has since been moved or deleted must not resurrect as a
/// broken tab, so a stale entry is dropped rather than surfaced.
pub struct Session {
    dir: PathBuf,
    current: Option<SessionState>,
}

impl Session {
    /// `restore` is false for every window but the first: a new window starts empty.
    pub fn new(user_data: &Path, restore: bool) -> Self {
        Self { dir: user_data.to_path_buf(), current: (!restore).then(SessionState::default) }
    }

    fn marker(&self) -> PathBuf {
        self.dir.join(".first-run-done")
    }

    /// True until the app has completed one launch. A fresh install shows the
    /// welcome document rather than an empty window.
    pub fn is_first_run(&self) -> bool {
        !self.marker().exists()
    }

    pub fn mark_run(&self) -> Result<(), String> {
        fs::write(self.marker(), chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()).map_err(io_err)
    }

    pub fn load(&mut self) -> &SessionState {
        if self.current.is_none() {
            let saved = fs::read_to_string(self.dir.join("session.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
            self.current = Some(saved.map(prune).unwrap_or_default());
        }
        self.current.as_ref().unwrap()
    }

    /// `load`, with `first_run` reported once and then never again.
    pub fn boot(&mut self) -> Result<SessionState, String> {
        let mut state = self.load().clone();
        let first = self.is_first_run();
        if first {
            self.mark_run()?;
        }
        state.first_run = Some(first);
        Ok(state)
    }

    /// Save the state and remember its workspace among the recent ones.
    pub fn save(&mut self, mut state: SessionState) -> Result<(), String> {
        state.updated_at = now_ms();
        let json = serde_json::to_string(&state).map_err(|e| e.to_string())?;
        let root = state.workspace_root.clone();
        self.current = Some(state);
        fs::write(self.dir.join("session.json"), json).map_err(io_err)?;
        match root {
            Some(root) => self.remember_root(&root),
            None => Ok(()),
        }
    }

    fn recent_file(&self) -> PathBuf {
        self.dir.join("recent.json")
    }

    /// Recently opened workspaces, newest first, existing ones only.
    pub fn recent_roots(&self, limit: usize) -> Vec<PathBuf> {
        #[derive(Deserialize)]
        struct Recent {
            #[serde(default)]
            roots: Vec<PathBuf>,
        }
        let Some(recent) = fs::read_to_string(self.recent_file()).ok().and_then(|s| serde_json::from_str::<Recent>(&s).ok()) else { return vec![] };
        recent.roots.into_iter().filter(|r| r.exists()).take(limit).collect()
    }

    pub fn remember_root(&self, root: &Path) -> Result<(), String> {
        let _held = RECENT.lock().unwrap_or_else(|e| e.into_inner());
        let mut roots = vec![root.to_path_buf()];
        roots.extend(self.recent_roots(50).into_iter().filter(|r| r != root));
        roots.truncate(12);
        fs::write(self.recent_file(), serde_json::json!({ "roots": roots }).to_string()).map_err(io_err)
    }
}

/// Drop anything that no longer exists before a window ever sees it.
fn prune(mut state: SessionState) -> SessionState {
    state.open_docs.retain(|p| p.exists());
    if !state.active_doc.as_ref().is_some_and(|p| p.exists()) {
        state.active_doc = state.open_docs.first().cloned();
    }
    if state.workspace_root.as_ref().is_some_and(|r| !r.exists()) {
        state.workspace_root = None;
        state.active_doc = None;
        state.open_docs.clear();
    }
    let gone: Vec<_> = state.cursors.keys().filter(|k| !k.exists()).cloned().collect();
    for k in gone {
        state.cursors.remove(&k);
        state.scrolls.remove(&k);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_each_window_session_across_reloads_and_restores_the_last_saved_session_on_restart() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join("doc.md");
        fs::write(&path, "# First").unwrap();
        let mut first = Session::new(root, true);
        first.save(SessionState { active_doc: Some(path.clone()), workspace_root: Some(root.into()), ..Default::default() }).unwrap();
        let mut second = Session::new(root, false);
        assert_eq!(second.load().active_doc, None);
        second.save(SessionState::default()).unwrap();
        assert_eq!(first.load().active_doc, Some(path.clone()));
        first.remember_root(root).unwrap();
        second.remember_root(other.path()).unwrap();
        assert_eq!(first.recent_roots(8), [other.path().to_path_buf(), root.to_path_buf()]);
        first.save(SessionState { active_doc: Some(path.clone()), ..Default::default() }).unwrap();
        assert_eq!(Session::new(root, true).load().active_doc, Some(path));
    }

    #[test]
    fn prunes_paths_that_no_longer_exist_and_reports_first_run_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let alive = root.join("a.md");
        fs::write(&alive, "").unwrap();
        let gone = root.join("gone.md");
        let json = serde_json::json!({
            "workspaceRoot": root, "activeDoc": gone, "openDocs": [gone, alive],
            "cursors": { gone.to_str().unwrap(): 3, alive.to_str().unwrap(): 4 }, "scrolls": { gone.to_str().unwrap(): 1.5 }, "updatedAt": 1
        });
        fs::write(root.join("session.json"), json.to_string()).unwrap();
        let mut s = Session::new(root, true);
        let state = s.boot().unwrap();
        assert_eq!((state.active_doc.clone(), state.open_docs.clone()), (Some(alive.clone()), vec![alive.clone()]));
        assert_eq!((state.cursors.len(), state.scrolls.len(), state.first_run), (1, 0, Some(true)));
        assert_eq!(s.boot().unwrap().first_run, Some(false));
    }
}
