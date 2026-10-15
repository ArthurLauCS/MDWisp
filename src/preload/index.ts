import { contextBridge, ipcRenderer, webUtils } from 'electron'
import { IPC } from '@shared/ipc'
import type {
  AppSettings,
  AssetRef,
  DocumentContent,
  DocumentMeta,
  ExportMode,
  ExportPreview,
  InsertedAsset,
  LinkTarget,
  OrganizeOptions,
  OrganizePlan,
  DiffSummary,
  Revision,
  SessionState,
  WindowLaunch,
  WorkspaceInfo
} from '@shared/types'

const api = {
  window: {
    open: (path: string | null, fragment = ''): Promise<void> => ipcRenderer.invoke(IPC.WINDOW_OPEN, path, fragment),
    minimize: () => ipcRenderer.send(IPC.WINDOW_MINIMIZE),
    toggleMaximize: () => ipcRenderer.send(IPC.WINDOW_TOGGLE_MAXIMIZE),
    close: () => ipcRenderer.send(IPC.WINDOW_CLOSE),
    confirmClose: () => ipcRenderer.send(IPC.WINDOW_CONFIRM_CLOSE),
    onCloseRequested: (callback: () => void): (() => void) => {
      ipcRenderer.on(IPC.WINDOW_REQUEST_CLOSE, callback)
      return () => { ipcRenderer.removeListener(IPC.WINDOW_REQUEST_CLOSE, callback) }
    },
    isMaximized: (): Promise<boolean> => ipcRenderer.invoke(IPC.WINDOW_IS_MAXIMIZED)
  },
  workspace: {
    openDialog: (): Promise<WorkspaceInfo | null> => ipcRenderer.invoke(IPC.WORKSPACE_OPEN),
    openPath: (path: string): Promise<WorkspaceInfo> =>
      ipcRenderer.invoke(IPC.WORKSPACE_OPEN_PATH, path),
    tree: (): Promise<WorkspaceInfo | null> => ipcRenderer.invoke(IPC.WORKSPACE_TREE)
  },
  doc: {
    resolveLink: (docPath: string, href: string): Promise<LinkTarget> => ipcRenderer.invoke(IPC.DOC_RESOLVE_LINK, docPath, href),
    read: (path: string): Promise<DocumentContent> => ipcRenderer.invoke(IPC.DOC_READ, path),
    watch: (path: string): Promise<void> => ipcRenderer.invoke(IPC.DOC_WATCH, path),
    onChangedOnDisk: (callback: () => void): (() => void) => {
      ipcRenderer.on(IPC.DOC_CHANGED_ON_DISK, callback)
      return () => { ipcRenderer.removeListener(IPC.DOC_CHANGED_ON_DISK, callback) }
    },
    write: (path: string, text: string, expected?: string): Promise<DocumentMeta> =>
      ipcRenderer.invoke(IPC.DOC_WRITE, path, text, expected),
    create: (dir: string, stem: string, withAssetFolder: boolean): Promise<DocumentMeta> =>
      ipcRenderer.invoke(IPC.DOC_CREATE, dir, stem, withAssetFolder),
    rename: (path: string, nextStem: string): Promise<DocumentMeta> =>
      ipcRenderer.invoke(IPC.DOC_RENAME, path, nextStem),
    remove: (path: string): Promise<boolean> => ipcRenderer.invoke(IPC.DOC_DELETE, path),
    organizePlan: (path: string, text: string): Promise<OrganizePlan> =>
      ipcRenderer.invoke(IPC.DOC_ORGANIZE_PLAN, path, text),
    /** Resolves to the new document path and the remote images that could not be fetched. */
    organize: (path: string, text: string, options: OrganizeOptions): Promise<{ docPath: string; failed: string[] }> =>
      ipcRenderer.invoke(IPC.DOC_ORGANIZE, path, text, options)
  },
  asset: {
    fromClipboard: (docPath: string): Promise<InsertedAsset | null> =>
      ipcRenderer.invoke(IPC.ASSET_FROM_CLIPBOARD, docPath),
    fromFiles: (docPath: string, paths: string[]): Promise<InsertedAsset[]> =>
      ipcRenderer.invoke(IPC.ASSET_FROM_FILES, docPath, paths),
    /**
     * Electron 32 deprecated `File.path` and 33 removed it. `webUtils` is the
     * supported replacement, and it only exists in the preload, so resolving
     * a dropped file to a real path has to happen on this side of the bridge.
     */
    pathForFile: (file: File): string | null => {
      try {
        return webUtils.getPathForFile(file) || null
      } catch {
        return null
      }
    },
    /** Drag payloads whose bytes only exist in the renderer (rare, but real). */
    saveBuffer: (docPath: string, name: string, bytes: Uint8Array): Promise<InsertedAsset> =>
      ipcRenderer.invoke(IPC.ASSET_SAVE_BUFFER, docPath, name, bytes),
    list: (docPath: string, text: string): Promise<AssetRef[]> =>
      ipcRenderer.invoke(IPC.ASSET_LIST, docPath, text),
    orphans: (): Promise<string[]> => ipcRenderer.invoke(IPC.ASSET_ORPHANS),
    dataUrl: (path: string): Promise<string> => ipcRenderer.invoke(IPC.ASSET_READ_DATA_URL, path),
    reveal: (path: string) => ipcRenderer.send(IPC.ASSET_REVEAL, path)
  },
  history: {
    list: (docId: string): Promise<Revision[]> => ipcRenderer.invoke(IPC.HISTORY_LIST, docId),
    read: (docId: string, revId: string): Promise<string | null> =>
      ipcRenderer.invoke(IPC.HISTORY_READ, docId, revId),
    record: (
      docId: string,
      text: string,
      kind: Revision['kind']
    ): Promise<Revision | null> => ipcRenderer.invoke(IPC.HISTORY_RECORD, docId, text, kind),
    forget: (docId: string, revId: string): Promise<boolean> =>
      ipcRenderer.invoke(IPC.HISTORY_FORGET, docId, revId),
    clear: (docId: string): Promise<boolean> => ipcRenderer.invoke(IPC.HISTORY_CLEAR, docId),
    diff: (docId: string, a: string, b: string): Promise<DiffSummary | null> =>
      ipcRenderer.invoke(IPC.HISTORY_DIFF, docId, a, b)
  },
  session: {
    load: (): Promise<SessionState> => ipcRenderer.invoke(IPC.SESSION_LOAD),
    save: (state: Partial<SessionState>): Promise<boolean> =>
      ipcRenderer.invoke(IPC.SESSION_SAVE, state),
    recent: (): Promise<string[]> => ipcRenderer.invoke(IPC.SESSION_RECENT)
  },
  app: {
    /** Reveal a bundled file in Explorer. Returns null when it is absent. */
    revealStock: (name: string): Promise<string | null> =>
      ipcRenderer.invoke(IPC.APP_REVEAL_STOCK, name),
    /**
     * Path to a bundled document, copied into the user's documents on first
     * use. A packaged app's resources directory is read-only and the document
     * needs somewhere writable for the images it will gain.
     */
    resolveStock: (name: string): Promise<string | null> =>
      ipcRenderer.invoke(IPC.APP_RESOLVE_STOCK, name),
    pluginPath: (): Promise<string> => ipcRenderer.invoke(IPC.APP_PLUGIN_PATH),
    installSkills: (): Promise<string | null> => ipcRenderer.invoke(IPC.APP_INSTALL_SKILLS),
    launchDocument: (): Promise<WindowLaunch> => ipcRenderer.invoke(IPC.APP_LAUNCH_DOCUMENT)
  },
  export: {
    preview: (mode: ExportMode, docPath: string, text: string): Promise<ExportPreview> =>
      ipcRenderer.invoke(IPC.EXPORT_PREVIEW, mode, docPath, text),
    run: (mode: ExportMode, docPath: string, text: string): Promise<string | null> =>
      ipcRenderer.invoke(IPC.EXPORT_RUN, mode, docPath, text)
  },
  settings: {
    onChanged: (callback: (settings: AppSettings) => void): (() => void) => {
      const listener = (_event: unknown, settings: AppSettings): void => callback(settings)
      ipcRenderer.on(IPC.SETTINGS_CHANGED, listener)
      return () => { ipcRenderer.removeListener(IPC.SETTINGS_CHANGED, listener) }
    },
    get: (): Promise<AppSettings> => ipcRenderer.invoke('settings:get'),
    patch: (patch: Partial<AppSettings>): Promise<AppSettings> =>
      ipcRenderer.invoke('settings:patch', patch)
  },
  shell: {
    openExternal: (url: string): Promise<void> => ipcRenderer.invoke(IPC.SHELL_OPEN_EXTERNAL, url)
  },
  files: {
    /** Write a text file at a path the user has already chosen. */
    write: (targetPath: string, text: string): Promise<string> =>
      ipcRenderer.invoke(IPC.FILE_WRITE_SIBLING, targetPath, text)
  },
  clipboard: {
    readTable: (): Promise<string> => ipcRenderer.invoke(IPC.CLIPBOARD_READ_TABLE),
    writeText: (text: string): Promise<boolean> =>
      ipcRenderer.invoke(IPC.CLIPBOARD_WRITE_TEXT, text),
    writeImage: (path: string): Promise<boolean> =>
      ipcRenderer.invoke(IPC.CLIPBOARD_WRITE_IMAGE, path)
  },
  dialog: {
    openDocument: (): Promise<string | null> => ipcRenderer.invoke(IPC.DIALOG_OPEN_DOCUMENT),
    confirmSave: (name: string): Promise<'save' | 'discard' | 'cancel'> => ipcRenderer.invoke(IPC.DIALOG_CONFIRM_SAVE, name),
    openImages: (): Promise<string[] | null> => ipcRenderer.invoke(IPC.DIALOG_OPEN_FILE),
    /** Save-as dialog. Returns null when the user cancels. */
    saveFile: (
      defaultName: string,
      filters?: { name: string; extensions: string[] }[]
    ): Promise<string | null> => ipcRenderer.invoke(IPC.DIALOG_SAVE_FILE, defaultName, filters)
  }
}

export type MdViewApi = typeof api

contextBridge.exposeInMainWorld('mdview', api)
