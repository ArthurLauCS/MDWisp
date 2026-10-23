<div align="center">
  <img src="build/icon.svg" width="112" height="112" alt="MDWisp icon" />

# MDWisp

[English](README.md) · [简体中文](README.zh-CN.md)

**Start writing. Keep images with your document.**

A local Markdown desktop editor for writing, code notes and tables, with English and Simplified Chinese interfaces.

[Download](https://github.com/ArthurLauCS/MDWisp/releases/latest) · [AI skills](#ai-skills) · [Getting started](#getting-started) · [Changelog](CHANGELOG.md)

</div>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/editor-light.png" />
  <img src="docs/images/editor-dark.png" alt="Editor with outline, live Markdown, code highlighting and tables" width="100%" />
</picture>

Formerly **MDView**. Existing settings and history remain compatible. The repository and new AI skill installations use MDWisp. The screenshots show the 0.4 Chinese interface; 0.5 keeps the same layout.

Since 0.5 MDWisp is a native Windows app written in Rust: the window appears in about 90 ms and uses about 60 MB of memory in one process (0.4 on Electron: about 850 ms, 450 MB, four processes).

## Features

| Write | Organize | Share |
| --- | --- | --- |
| Edit directly in formatted Markdown | Collapsible heading outline | Markdown, ZIP, HTML and PDF export |
| Adjustable margins, sidebar widths and outline/image split | Relative image paths beside the document | HTML with embedded images and fonts |
| Local code detection and explicit language selection | Version history, comparison and restore | Rules for Codex, Cursor and Claude Code |
| Dark/light themes and configurable fonts | Table editing, sorting, statistics and transpose | Ordinary files that work in other editors |

## Download and installation

Download `MDWisp-0.5.2-setup.exe` from [Releases](https://github.com/ArthurLauCS/MDWisp/releases/latest), or build it yourself (see [Development](#development-and-verification)). It needs Windows 10 or later, x64. Installation offers English and Simplified Chinese, a directory choice and a desktop shortcut. It installs for the current user without administrator rights, and removes an installed 0.4.x version while keeping its settings and history.

The installer is unsigned, so Windows SmartScreen may show "Windows protected your PC". Choose **More info → Run anyway**. macOS and Linux are not supported.

`MDWisp-AI-skills-<version>.zip` provides AI rules without installing the app. Copy `en/.agents` into a project for English rules, or `.agents` for Simplified Chinese. Check downloaded files against `SHA256SUMS.txt` from the release.

## Getting started

1. Start typing immediately. **Ctrl+N** creates a document in a new window; **Ctrl+O** opens one in a new window.
2. **Ctrl+S** chooses a name and location on first save. Canceling keeps the draft. Untitled drafts have no autosave or snapshots and must be saved before quitting.
3. Save before pasting, dropping or inserting images. Images are copied beside the document with relative links and listed in the sidebar.
4. Organize a loose file into a document folder, then use **Ctrl+Shift+E** to export.

New documents, opened files and repeated shortcut launches use independent windows, preserving the current document and its unsaved edits. A document that is already open is not opened twice: its window comes to the front. New windows open in front, one title bar below and right of the previous window so its title bar stays in reach. After the first save, autosave and history follow **Settings → Documents & saving**. Closing a window offers Save, Don't save and Cancel for unsaved edits. A failed save keeps that window open. A stale save cannot overwrite changes made by another window.

## Editing and layout

Use **Ctrl+/** to switch between formatted editing and Markdown source. **Ctrl+= / Ctrl+- / Ctrl+0** enlarge, reduce or reset the body text size. The status bar counts Chinese characters, English words and selected text.

Inline math uses `$...$`; display math uses `$$...$$`. The editor currently shows formulas as styled TeX source; HTML/PDF exports render them as MathML. Select text and paste a URL to turn it into a link. In tables, **Enter / Shift+Enter** move to the next or previous row while long cells remain multi-line.

For saved documents, drag the divider above the images panel to adjust its height relative to the outline or file tree. Drag to either edge to hide that section, then drag back to restore it. Double-click resets the split; a focused divider supports **Up / Down** keys. The split persists across restarts.

External edits reload automatically when the document has no unsaved changes. Otherwise a conflict notice appears; **Ctrl+Shift+R** loads the disk version and can be undone.

## Language and links

Use **Ctrl+F** to find text and **Ctrl+H** to replace it in the current document. The panel supports replace-next, replace-all, case sensitivity, whole words and regular expressions. **F3 / Shift+F3** cycle through matches; **Esc** closes search. Search keeps the formatted preview, with visible matches in tables, link destinations, image paths and hidden Markdown markers. Replacements can be undone; read-only documents allow searching only. Version history uses **Ctrl+Alt+Y**.

Choose **Settings → Appearance & fonts → Language** for **English** or **中文/Chinese**. The choice applies immediately and persists across restarts without translating document content or resetting the editor.

Use **Ctrl+click** to follow links. Holding the modifier displays a hand cursor over links. Relative Markdown files open in a new app window, heading anchors navigate within the current document, websites open in the default browser and email links use the mail handler. Reference links, table links and linked images are supported. Ordinary clicks remain editing actions.

```markdown
[English](README.md) · [简体中文](README.zh-CN.md)
[Website](https://example.com)
[Section](#getting-started)
```

`[English]\(README.md)` escapes the parenthesis and is literal text, not a Markdown link. See the [syntax support matrix](docs/markdown-support.md) for supported syntax and limitations.

## Portable document folders

```text
Notes/
├── Notes.md
└── Notes_img/
    └── architecture.svg
```

```markdown
![Architecture](./Notes_img/architecture.svg)
```

The folder name, Markdown filename and `_img` prefix match. Omit the image folder when there are no images. Share the entire document folder so images remain available on another machine.

**Organize into a document folder** previews local images, missing references and optional remote downloads. Keep the original or move it to the recycle bin. Code examples remain unchanged. ZIP export requires a document folder so unrelated files are not archived.

## Code, tables and history

Code fences use an explicit language or local rule-based detection without network access. Uncertain results remain plain text.

Click table cells to edit. Toolbar, context menu and palette actions support rows, columns, alignment, sorting, statistics, merging and conversion. Some registered actions remain unavailable; see [project status](docs/STATUS.md).

History opens beside the editor. Inspect snapshots, compare changes, record a snapshot or restore a version. Restoration saves the current content first and can be undone. Deleting snapshots or clearing history is permanent. History is stored in application data, outside shared document folders.

## Export

| Format | Result |
| --- | --- |
| Plain Markdown | Removes local images and paths according to the image policy, with a preview |
| ZIP | Archives saved files in the document folder without rewriting links |
| HTML | A self-contained file with local images and fonts embedded |
| PDF | A4 pages with embedded local images |

## AI skills

The `doc` skill creates portable document folders; `share` previews and removes local images and paths for a standalone file. All clients use the [plugin rule sources](plugin/mdwisp/skills), with English companion rules included.

In **Help → Rules for AI assistants**, choose a project to install skills for Codex and Cursor. Existing project instructions are preserved. Conflicting skill files must be backed up before replacing them. The selected interface language determines the installed rules' language.

| Client | Write | Share |
| --- | --- | --- |
| Codex | `$mdwisp-doc Write a user guide` | `$mdwisp-share Export standalone Markdown` |
| Cursor | `/mdwisp-doc Write a user guide` | `/mdwisp-share Export standalone Markdown` |

Skills install in `.agents/skills/mdwisp-doc/` and `.agents/skills/mdwisp-share/`. Previously installed `mdview-*` skills are left untouched; new installations use `mdwisp-*`. Alternatively copy `en/.agents` from the AI skills archive into your project as `.agents`; for all projects, use `~/.agents/skills/` and preserve custom modifications.

For Claude Code, use the app's **Copy install command**, or run from a checkout:

```bash
claude plugin marketplace add ./plugin
claude plugin install mdwisp@mdwisp
```

Use `/mdwisp:doc` or `/mdwisp:share` in a new session. Validate generated folders with Node.js:

```bash
node .agents/skills/mdwisp-doc/scripts/check.mjs "./Notes"
```

## Shortcuts

| Action | Shortcut | Action | Shortcut |
| --- | --- | --- | --- |
| New document | Ctrl+N | Open document | Ctrl+O |
| Save | Ctrl+S | Export | Ctrl+Shift+E |
| Command palette | Ctrl+P | Settings | Ctrl+, |
| History | Ctrl+Alt+Y | Code block | Ctrl+Shift+C |
| Insert image | Ctrl+Shift+I | Toggle sidebar | Ctrl+\ |
| Undo | Ctrl+Z | Redo | Ctrl+Shift+Z / Ctrl+Y |

Press **F1** for the searchable list. Table, code and image commands depend on focus. App commands such as Export remain available while panels are open.

## Known limitations

These differ from 0.4 and will be addressed in later versions:

- Math shows as TeX source in the editor; exports render it.
- The spell check setting has no effect yet.
- "Select all matches" is unavailable because the editor has a single selection.
- 14 table actions without shortcuts (table styles, min/max, Excel paste, CSV/JSON/list conversion, CSV export, delete table) are listed but disabled.
- Renaming a document can overwrite an existing file with the same name.

Details are in [project status](docs/STATUS.md) (Chinese).

## Development and verification

Written in Rust on Win32, Direct2D, DirectWrite and DirectComposition. Use stable Rust (MSVC toolchain) and Git; building the installer needs [Inno Setup 6](https://jrsoftware.org/isinfo.php). Node.js is only needed for the plugin's `check.mjs` test.

```bash
cargo test --workspace
cargo build --release -p mdwisp        # target/release/mdwisp.exe
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/dist.ps1   # target/installer/MDWisp-<version>-setup.exe
claude plugin validate plugin
```

UI regressions drive a real window with a script and save the app's own frames. Run them on document copies with an isolated profile; the commands are listed in `crates/app/src/script.rs`:

```bash
MDWISP_USER_DATA=<temp dir> MDWISP_SCRIPT=<script> target/release/mdwisp.exe <copy of a document>
```

See [AGENTS.md](AGENTS.md) (Chinese) for development rules. Report issues in the existing [GitHub repository](https://github.com/ArthurLauCS/MDWisp/issues).

## License and fonts

Code uses [MIT](LICENSE). Bundled fonts retain their open licenses: [GenSen Rounded](crates/export/assets/fonts/OFL-GenSen.txt) and [ZCOOL XiaoWei](crates/export/assets/fonts/OFL-ZCOOLXiaoWei.txt).
