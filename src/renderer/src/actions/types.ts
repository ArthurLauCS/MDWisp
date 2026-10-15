/**
 * Actions are the single source of truth for what the app can do.
 *
 * Keyboard shortcuts, the command palette and the context menu are all
 * *views* over this registry. Nothing binds a key or draws a menu item
 * directly — that is how the three surfaces stay in sync for free.
 */

import type { SearchCommand } from '../editor/search'

export type ActionScope =
  /** Works anywhere, even with nothing open. */
  | 'global'
  /** Needs an open document. */
  | 'document'
  /**
   * Always available, even with no document open. Separate from `document`
   * so a right-click on empty chrome still offers clipboard and view items.
   */
  | 'app'
  /** Only meaningful while a text selection exists. */
  | 'selection'
  /** Text formatting that applies with or without a selection. */
  | 'format'
  /** Block-type conversion: headings, lists, quotes. */
  | 'convert'
  /** Opening or inserting something new. */
  | 'insert'
  /** Search and navigation within the document. */
  | 'find'
  /** Copy and paste variants. */
  | 'clipboard'
  /** Cursor is inside a fenced code block. */
  | 'codeblock'
  /** Cursor is inside a table — the largest group by design. */
  | 'table'
  /** Cursor is inside a table cell whose column can be acted on. */
  | 'tableColumn'
  /** A rendered image element was clicked. */
  | 'image'
  /** A hyperlink was clicked. */
  | 'link'
  /** Focus is in the sidebar file tree. */
  | 'filetree'

export interface KeyBinding {
  /** e.g. `Ctrl+B`, `Alt+ArrowUp`, `Ctrl+Shift+Digit1`. */
  key: string
  /** Shown in menus; falls back to a prettified `key`. */
  label?: string
}

export interface ActionDef {
  id: string
  /** Menu / palette label, in the user's language. */
  title: string
  /** Extra search terms so the palette finds it by the English name too. */
  keywords?: string[]
  scope: ActionScope
  /** Primary binding. Also the one printed in the context menu. */
  key?: string
  /** Additional bindings, e.g. a second layout for the same operation. */
  altKeys?: string[]
  /** Grouping inside the context menu. Order comes from the group list. */
  group: string
  /**
   * When false the item renders disabled. Receives the live editor context so
   * an item can explain itself, e.g. "合并单元格：需选择 ≥2 格".
   */
  enabled?: (ctx: ActionContext) => boolean
  /** Returned by `enabled` via `disabledReason` for the tooltip. */
  disabledReason?: (ctx: ActionContext) => string | undefined
  run: (ctx: ActionContext) => void | Promise<void>
}

export interface ActionContext {
  /** Available only while a rendered image has keyboard focus. */
  fullscreenImage?: () => void
  /** Raw source of the open document, or '' when none. */
  source: string
  /** Current selection start, as a character offset. */
  cursor: number
  selection: { start: number; end: number } | null
  docPath: string | null
  /** True when the cursor sits inside a fenced code block. */
  inCodeBlock: boolean
  inTable: boolean
  /** Column index when `inTable`, else null. */
  tableColumn: number | null
  /** Replace a range in the source and place the cursor. */
  replace: (start: number, end: number, text: string, cursor?: number) => void
  /** Wrap each selected line with a prefix, or unwrap if already present. */
  toggleLinePrefix: (prefix: string | ((index: number) => string)) => void
  /** Toggle an inline wrapper such as `**` around the selection. */
  toggleInline: (wrap: string) => void
  /** Insert text at the cursor on its own line, adding blank lines if needed. */
  insertBlock: (text: string) => void
  /** Select the given range — used by table actions after a structural edit. */
  select: (start: number, end: number) => void
  jump?: (offset: number) => void
  /** Set focus back into the editor. */
  focus: () => void
  save?: () => void | Promise<void>
  undo?: () => void
  redo?: () => void
  search?: (command: SearchCommand) => void
  searchOpen?: () => boolean
  pickCodeLanguage?: () => void
  /** Replace the buffer with the file on disk as one undoable step. */
  reloadFromDisk?: () => void | Promise<void>
  /** Push a transient message. */
  toast: (message: string, tone?: 'info' | 'success' | 'error') => void
}
