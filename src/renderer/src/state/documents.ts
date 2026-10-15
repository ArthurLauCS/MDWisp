import { t } from '../i18n'
import { useEffect, useState } from 'react'
import type { DocumentContent } from '@shared/types'
import { settingsSnapshot } from './settings'
import { recordRevision } from '../history/revisions'

let state: { active: DocumentContent | null; dirty: boolean; confirming: boolean; transitioning: boolean; key: number; error: string | null; fragment: string } = {
  active: null, dirty: false, confirming: false, transitioning: false, key: 0, error: null, fragment: ''
}
let buffer = ''
let saved = ''
let saving: Promise<boolean> | null = null
let changing: Promise<void> | null = null
const listeners = new Set<() => void>()

function publish(patch: Partial<typeof state>): void {
  state = { ...state, ...patch }
  for (const listener of listeners) listener()
}

export function documentError(error: unknown): void {
  publish({ error: error instanceof Error ? error.message : String(error) })
}

export function updateDocumentBuffer(text: string): void {
  buffer = text
  if (state.dirty !== (buffer !== saved)) publish({ dirty: buffer !== saved })
}

export function documentBuffer(): string { return buffer }
export function currentDocument(): DocumentContent | null { return state.active }
export function hasUnsavedChanges(): boolean { return state.dirty || saving !== null }

function activate(document: DocumentContent, fragment = ''): void {
  buffer = saved = document.text
  publish({ active: document, dirty: false, key: state.key + 1, error: null, fragment })
}

export function saveDocument(manual = true): Promise<boolean> {
  if (saving) return saving
  const document = state.active
  if (!document) return Promise.resolve(false)
  saving = (async () => {
    try {
      const path = document.meta.path || await window.mdview.dialog.saveFile(t('未命名.md'), [
        { name: t('Markdown 文档'), extensions: ['md', 'markdown'] }
      ])
      if (!path) return false
      const text = buffer
      const meta = await window.mdview.doc.write(path, text, document.meta.path ? saved : undefined)
      saved = text
      // Keep the editor instance and newer keystrokes when a draft gains a path.
      publish({ active: { ...document, meta, text: buffer, body: buffer }, dirty: buffer !== saved, error: null })
      if (manual && settingsSnapshot().historyEnabled) {
        await recordRevision(meta.id, text, 'manual').catch((error) => documentError(t('文件已保存，历史记录失败：{0}', error)))
      }
      return true
    } catch (error) {
      documentError(t('保存失败：{0}', error instanceof Error ? error.message : error))
      return false
    }
  })().finally(() => { saving = null })
  return saving
}

/** The file as it is on disk now, and whether it moved on since this window last loaded or saved it. */
export async function diskState(): Promise<{ text: string; changed: boolean; dirty: boolean } | null> {
  if (saving) await saving
  const path = state.active?.meta.path
  if (!path) return null
  const { text } = await window.mdview.doc.read(path)
  return { text, changed: text !== saved, dirty: state.dirty }
}

/** Make the disk text the saved baseline before the editor switches to it. */
export function adoptDiskText(text: string): void {
  saved = text
  publish({ error: null })
  updateDocumentBuffer(buffer)
}

export async function confirmDocumentChange(): Promise<boolean> {
  if (saving) await saving
  if (!state.dirty) return true
  publish({ confirming: true })
  try {
    const choice = await window.mdview.dialog.confirmSave(state.active!.meta.path ? state.active!.meta.stem : t('未命名'))
    if (choice === 'cancel') return false
    if (choice === 'discard') return true
    return await saveDocument() && !state.dirty
  } finally { publish({ confirming: false }) }
}

function change(action: () => Promise<void>): Promise<void> {
  if (changing) return changing
  publish({ transitioning: true })
  changing = action().catch(documentError).finally(() => { changing = null; publish({ transitioning: false }) })
  return changing
}

export function initializeDocument(): Promise<void> {
  return change(async () => {
    if (!await confirmDocumentChange()) return
    activate({
      meta: { id: crypto.randomUUID(), path: '', parentDir: '', stem: t('未命名'), assetDir: '', inFolder: false },
      text: '', body: '', frontmatter: null, conflictWithDisk: false
    })
  })
}

export function loadDocument(path: string, fragment = ''): Promise<void> {
  return change(async () => {
    if (state.active?.meta.path === path) return
    if (!await confirmDocumentChange()) return
    activate(await window.mdview.doc.read(path), fragment)
  })
}

export function newDocument(): Promise<void> {
  return window.mdview.window.open(null).catch(documentError)
}

export function openDocument(path: string, fragment = ''): Promise<void> {
  return window.mdview.window.open(path, fragment).catch(documentError)
}

export async function openDocumentDialog(): Promise<void> {
  const path = await window.mdview.dialog.openDocument()
  if (path) await openDocument(path)
}

export function closeDocument(): void { window.mdview.window.close() }

export function useDocuments() {
  const [value, setValue] = useState(state)
  useEffect(() => {
    const listener = (): void => setValue(state)
    listeners.add(listener)
    listener()
    return () => { listeners.delete(listener) }
  }, [])
  return { ...value, open: openDocument, close: closeDocument }
}
