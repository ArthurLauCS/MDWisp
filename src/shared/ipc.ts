/**
 * Single source of truth for every IPC channel name.
 * Main and preload both import from here, so a typo is a compile error.
 */
export const IPC = {
  // ---- window / app shell -------------------------------------------------
  WINDOW_MINIMIZE: 'window:minimize',
  WINDOW_OPEN: 'window:open',
  WINDOW_TOGGLE_MAXIMIZE: 'window:toggle-maximize',
  WINDOW_CLOSE: 'window:close',
  WINDOW_REQUEST_CLOSE: 'window:request-close',
  WINDOW_CONFIRM_CLOSE: 'window:confirm-close',
  WINDOW_IS_MAXIMIZED: 'window:is-maximized',
  WINDOW_MAXIMIZE_CHANGED: 'window:maximize-changed',

  // ---- workspace ----------------------------------------------------------
  WORKSPACE_OPEN: 'workspace:open',
  WORKSPACE_OPEN_PATH: 'workspace:open-path',
  WORKSPACE_TREE: 'workspace:tree',
  WORKSPACE_TREE_CHANGED: 'workspace:tree-changed',
  WORKSPACE_RECENT: 'workspace:recent',

  // ---- documents ----------------------------------------------------------
  DOC_READ: 'doc:read',
  DOC_RESOLVE_LINK: 'doc:resolve-link',
  DOC_WRITE: 'doc:write',
  DOC_CREATE: 'doc:create',
  DOC_RENAME: 'doc:rename',
  DOC_DELETE: 'doc:delete',
  DOC_MOVE: 'doc:move',
  DOC_ORGANIZE_PLAN: 'doc:organize-plan',
  DOC_ORGANIZE: 'doc:organize',
  DOC_CHANGED_ON_DISK: 'doc:changed-on-disk',
  /** Watch this window's document so edits by other programs reach the editor. */
  DOC_WATCH: 'doc:watch',

  // ---- assets -------------------------------------------------------------
  ASSET_FROM_CLIPBOARD: 'asset:from-clipboard',
  ASSET_FROM_FILES: 'asset:from-files',
  ASSET_SAVE_BUFFER: 'asset:save-buffer',
  ASSET_LIST: 'asset:list',
  ASSET_ORPHANS: 'asset:orphans',
  ASSET_REVEAL: 'asset:reveal',
  ASSET_OPEN_EXTERNAL: 'asset:open-external',
  ASSET_READ_DATA_URL: 'asset:read-data-url',

  // ---- history ------------------------------------------------------------
  HISTORY_LIST: 'history:list',
  HISTORY_READ: 'history:read',
  HISTORY_RECORD: 'history:record',
  HISTORY_FORGET: 'history:forget',
  HISTORY_CLEAR: 'history:clear',
  HISTORY_DIFF: 'history:diff',

  // ---- session ------------------------------------------------------------
  SESSION_LOAD: 'session:load',
  SESSION_SAVE: 'session:save',
  SESSION_RECENT: 'session:recent',

  // ---- export -------------------------------------------------------------
  EXPORT_PREVIEW: 'export:preview',
  EXPORT_RUN: 'export:run',

  // ---- misc ---------------------------------------------------------------
  SHELL_OPEN_EXTERNAL: 'shell:open-external',
  SHELL_SHOW_ITEM: 'shell:show-item',
  CLIPBOARD_WRITE_TEXT: 'clipboard:write-text',
  CLIPBOARD_READ_TABLE: 'clipboard:read-table',
  CLIPBOARD_WRITE_IMAGE: 'clipboard:write-image',
  DIALOG_OPEN_FILE: 'dialog:open-file',
  DIALOG_OPEN_DOCUMENT: 'dialog:open-document',
  DIALOG_CONFIRM_SAVE: 'dialog:confirm-save',
  DIALOG_SAVE_FILE: 'dialog:save-file',
  /** Write text beside the document without asking — used by table exports. */
  FILE_WRITE_SIBLING: 'file:write-sibling',
  /** Reveal a bundle that shipped with the app, e.g. the welcome document. */
  APP_REVEAL_STOCK: 'app:reveal-stock',
  /** Resolve a bundled file, copying it into the user's documents on first use. */
  APP_RESOLVE_STOCK: 'app:resolve-stock',
  /** Where the Claude Code plugin shipped with the app lives on this machine. */
  APP_PLUGIN_PATH: 'app:plugin-path',
  APP_INSTALL_SKILLS: 'app:install-skills',
  /** The document this launch was started with, e.g. by a double click. */
  APP_LAUNCH_DOCUMENT: 'app:launch-document',
  SETTINGS_CHANGED: 'settings:changed'
} as const

export type IpcChannel = (typeof IPC)[keyof typeof IPC]
