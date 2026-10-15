import { t } from '../i18n'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Editor, type EditorHandle } from '../editor/Editor'
import { ContextMenu, type MenuAnchor } from './ContextMenu'
import { publishEditorContext } from '../state/editor-context'
import { clearLiveText, publishLiveText } from '../history/live-text'
import { recordRevision } from '../history/revisions'
import { TableToolbar } from '../tableui/TableToolbar'
import { useTableEdit } from '../tableui/useTableEdit'
import { insertFromClipboard, insertFromPaths } from '../assets/insert'
import { installDropTarget } from '../assets/drop'
import { codeAt } from '../actions/registry'
import { pasteAsLink } from '../actions/markdown-ops'
import { findTableAt } from '../table/model'
import { followDocumentLink, jumpToFragment } from '../markdown/navigation'
import { linkAt } from '../markdown/links'
import { adoptDiskText, diskState, documentBuffer, documentError, saveDocument, updateDocumentBuffer, useDocuments } from '../state/documents'
import { openPanel } from '../state/ui'
import { useSettings } from '../state/settings'
import type { ActionContext, ActionScope } from '../actions/types'
import './editorpane.css'

export function EditorPane(): JSX.Element {
  const { active, dirty, confirming, transitioning, fragment } = useDocuments()
  const settings = useSettings()
  const [source, setSource] = useState(documentBuffer)
  /**
   * The live buffer, readable and writable from outside this component.
   *
   * The history panel needs the *unsaved* text to snapshot it and to restore
   * over it, and it is mounted above the editor. A ref keeps that seam narrow
   * — the alternative, threading a callback through the panel host, would
   * make the panel depend on the editor's whole state shape.
   */
  const sourceRef = useRef(source)
  sourceRef.current = source
  const [menu, setMenu] = useState<MenuAnchor | null>(null)
  const [toast, setToast] = useState<string | null>(null)
  const [caret, setCaret] = useState({ cursor: 0 })
  const cursor = caret.cursor
  const editorRef = useRef<EditorHandle>(null)
  const paneRef = useRef<HTMLDivElement>(null)
  const docPath = active?.meta.path || null
  const tableEdit = useTableEdit(source, cursor)

  const showToast = (message: string): void => {
    setToast(message)
    window.setTimeout(() => setToast(null), 2200)
  }

  /** Undoable where the editor accepts edits, so Ctrl+Z brings this window's text back. */
  const showDiskText = (text: string): void => {
    adoptDiskText(text)
    editorRef.current?.setSource(text)
    if (editorRef.current?.getSource() !== text) update(text)
  }

  const reloadFromDisk = async (): Promise<void> => {
    const disk = await diskState()
    if (!disk) return
    if (disk.text === editorRef.current?.getSource()) { showToast(t('已是磁盘上的最新内容')); return }
    showDiskText(disk.text)
    showToast(t('已载入磁盘版本，按 Ctrl+Z 可找回之前的内容'))
  }

  useEffect(() => {
    if (!docPath) return
    void window.mdview.doc.watch(docPath)
    return window.mdview.doc.onChangedOnDisk(() => {
      void diskState().then((disk) => {
        if (!disk?.changed) return
        if (disk.dirty) documentError(t('文件已被其他程序修改。按 Ctrl+Shift+R 载入磁盘版本，载入后可按 Ctrl+Z 找回本窗口的修改。'))
        else {
          showDiskText(disk.text)
          showToast(t('已载入其他程序对文件的修改'))
        }
      }).catch(documentError)
    })
  }, [docPath])

  const ctx: ActionContext = useMemo(() => {
    const handle = editorRef.current
    const selection = handle?.getSelection() ?? null
    return {
      source,
      cursor,
      selection,
      docPath,
      inCodeBlock: codeAt(source, cursor) !== null,
      inTable: tableEdit.context !== null,
      tableColumn: tableEdit.cell?.col ?? null,
      replace: (start, end, text, caret) => editorRef.current?.replace(start, end, text, caret),
      save: async () => { await saveDocument() },
      undo: () => editorRef.current?.undo(),
      redo: () => editorRef.current?.redo(),
      search: (command) => editorRef.current?.search(command),
      searchOpen: () => editorRef.current?.searchOpen() ?? false,
      pickCodeLanguage: () => editorRef.current?.pickCodeLanguage(),
      reloadFromDisk,
      toggleLinePrefix: () => undefined,
      toggleInline: () => undefined,
      insertBlock: (block) => {
        const handle = editorRef.current
        const at = handle?.getCursor() ?? 0
        const text = handle?.getSource() ?? sourceRef.current
        const before = text.slice(0, at)
        const after = text.slice(at)
        const pad = before && !before.endsWith('\n\n') ? (before.endsWith('\n') ? '\n' : '\n\n') : ''
        handle?.replace(0, text.length, before + pad + block + after, at + pad.length + block.length)
      },
      select: (start, end) => editorRef.current?.select(start, end),
      jump: (offset) => editorRef.current?.jump(offset),
      focus: () => editorRef.current?.focus(),
      toast: (message) => {
        setToast(message)
        window.setTimeout(() => setToast(null), 2200)
      }
    }
  }, [source, caret, docPath, tableEdit.context, tableEdit.cell])

  const availableScopes = useMemo<Set<string>>(() => {
    const scopes = new Set<string>(['global', 'app', 'document', 'insert', 'clipboard'])
    if (linkAt(source, cursor) !== null) scopes.add('link')
    if (ctx.selection) {
      for (const s of ['selection', 'format', 'convert', 'find']) scopes.add(s)
    }
    if (ctx.inCodeBlock) scopes.add('codeblock')
    if (ctx.inTable) {
      scopes.add('table')
      scopes.add('tableColumn')
    }
    return scopes
  }, [ctx.selection, ctx.inCodeBlock, ctx.inTable, source, cursor])

  useEffect(() => {
    if (fragment) jumpToFragment(ctx, fragment)
  }, [])

  // Hand the live editing context up to the overlays mounted above this pane.
  useEffect(() => {
    // Native selection may move before React publishes its next render.
    publishEditorContext(() => {
      const handle = editorRef.current
      if (!handle) return ctx
      const source = handle.getSource()
      const cursor = handle.getCursor()
      return { ...ctx, source, cursor, selection: handle.getSelection(),
        inCodeBlock: codeAt(source, cursor) !== null, inTable: findTableAt(source, cursor) !== null }
    }, availableScopes)
    return () => publishEditorContext(null, new Set(['app', 'global']))
  }, [ctx, availableScopes])

  useEffect(() => {
    if (!docPath || !dirty || confirming || transitioning || !settings.autoSave) return
    const timer = window.setTimeout(() => { void saveDocument(false) }, settings.autoSaveDelayMs)
    return () => window.clearTimeout(timer)
  }, [source, docPath, active, dirty, confirming, transitioning, settings.autoSave, settings.autoSaveDelayMs])

  // Snapshot independently of disk autosave and typing pauses, so continuous
  // input and disabled autosave still leave restore points at the set interval.
  useEffect(() => {
    if (!docPath || !active || !settings.historyEnabled) return
    const snapshot = (): void => {
      void recordRevision(active.meta.id, sourceRef.current, 'auto')
        .catch((error) => setToast(t('历史记录失败：{0}', error.message)))
    }
    snapshot()
    const timer = window.setInterval(snapshot, settings.historyIntervalMs)
    return () => window.clearInterval(timer)
  }, [docPath, active?.meta.id, settings.historyEnabled, settings.historyIntervalMs])

  const update = useCallback(
    (next: string) => {
      sourceRef.current = next
      setSource(next)
      updateDocumentBuffer(next)
    },
    []
  )

  // Publish the buffer so the history panel can read it and write back over it.
  useEffect(() => {
    publishLiveText(
      () => sourceRef.current,
      (text) => editorRef.current?.setSource(text)
    )
    return () => clearLiveText()
  }, [update])

  /**
   * Pasting an image saves it beside the document and inserts a relative
   * link; pasting anything else falls through to the browser's own handling
   * so text, rich text and multi-line paste all behave normally.
   */
  /**
   * Accept images dropped anywhere on the pane. Text drags pass through
   * untouched — only an image payload is intercepted, which is what keeps
   * ordinary text drag-and-drop inside the editor working.
   */
  useEffect(() => {
    const el = paneRef.current
    if (!el || settings.readOnly) return
    return installDropTarget(
      el,
      () => docPath,
      (files) => {
        if (!docPath) { ctx.toast(t('请先保存文档，再插入图片；图片会放在文档旁边。')); return }
        const paths = files
          .map((f) => window.mdview.asset.pathForFile(f))
          .filter((p): p is string => p !== null)
        if (paths.length === 0) {
          setToast(t('这些文件无法定位到磁盘路径，请改用「插入图片」'))
          window.setTimeout(() => setToast(null), 3000)
          return
        }
        void insertFromPaths(docPath, source, editorRef.current?.getCursor() ?? 0, null, paths).then(
          (res) => {
            editorRef.current?.replace(0, editorRef.current.getSource().length, res.markdown, res.cursor)
          }
        )
      }
    )
  }, [docPath, source, settings.readOnly])

  const onPaste = useCallback(
    (e: ClipboardEvent) => {
      if (settings.readOnly || !e.clipboardData) return
      const hasImage = [...e.clipboardData.items].some((i) => i.type.startsWith('image/'))
      if (!hasImage) {
        const handle = editorRef.current
        const range = handle?.getSelection()
        const text = handle?.getSource() ?? ''
        const link = range && !codeAt(text, range.start) ? pasteAsLink(text.slice(range.start, range.end), e.clipboardData.getData('text/plain')) : null
        if (link) {
          e.preventDefault()
          handle!.replace(range!.start, range!.end, link)
        }
        return
      }
      e.preventDefault()
      if (!docPath) { ctx.toast(t('请先保存文档，再粘贴图片；图片会放在文档旁边。')); return }
      const handle = editorRef.current!
      const range = handle.getSelection()
      const text = handle.getSource()
      const selection = range ? text.slice(range.start, range.end) : undefined
      void insertFromClipboard(docPath, text, handle.getCursor(), selection).then(
        (res) => {
          if (!res) return
          editorRef.current?.replace(0, editorRef.current.getSource().length, res.markdown, res.cursor)
        }
      )
    },
    [docPath, source, settings.readOnly]
  )

  if (!active) return <></>

  return (
    <div className="ep">
      <div className="ep__toolbar">
        <div className="ep__meta">
          {dirty && <span className="ep__dirty" title={t('有未保存的改动')} />}
          <span className="ep__path" title={active.meta.path}>
            {docPath ? `${active.meta.parentDir.replace(/.*[\\/]/, '')} / ${active.meta.stem}` : t('未命名 · 首次保存时选择位置')}
          </span>
          {docPath && !active.meta.inFolder && (
            <button className="ep__loose" title={t('图片与文档分开存放，整个发给别人时会断')} onClick={() => openPanel('organize')}>
              {t('散装文件 · 整理为文档文件夹')}</button>
          )}
          <span className="ep__save-state">{!docPath ? t('尚未保存') : dirty ? t('有未保存修改') : t('已保存')}</span>
        </div>
      </div>

      <div className="ep__body" ref={paneRef}>
        <Editor
          language={settings.language}
          ref={editorRef}
          docPath={active.meta.path}
          source={source}
          onChange={update}
          onOpenLink={(href) => { void followDocumentLink(ctx, href) }}
          onCursorChange={(cursor) => setCaret({ cursor })}
          onPaste={onPaste}
          onContextMenu={(x, y) => setMenu({ x, y, target: { scope: menuScope(ctx) } })}
          readOnly={settings.readOnly || confirming || transitioning}
          typewriter={settings.typewriterMode}
          highlightLine={settings.highlightCurrentLine}
          spellCheck={settings.spellCheck}
          autoPair={settings.autoPair}
          smartLists={settings.smartLists}
          tabSize={settings.tabSize}
          sourceMode={settings.sourceMode}
        />
        {!settings.readOnly && <TableToolbar
          edit={tableEdit}
          onApply={(text, at) => editorRef.current?.replace(0, source.length, text, at)}
        />}
      </div>

      <ContextMenu anchor={menu} ctx={ctx} onClose={() => setMenu(null)} />
      {toast && <div className="ep__toast">{toast}</div>}
    </div>
  )
}

function menuScope(ctx: ActionContext): ActionScope {
  if (linkAt(ctx.source, ctx.cursor) !== null) return 'link'
  if (ctx.inCodeBlock) return 'codeblock'
  if (ctx.inTable) return 'table'
  if (ctx.selection) return 'selection'
  return 'document'
}
