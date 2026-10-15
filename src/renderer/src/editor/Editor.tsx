import { t } from '../i18n'
import { useEffect, useLayoutEffect, useImperativeHandle, useMemo, useRef, useState, forwardRef } from 'react'
import { Compartment, EditorState, Transaction } from '@codemirror/state'
import { EditorView, keymap, highlightActiveLine, placeholder, scrollPastEnd } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab, isolateHistory, undo, redo } from '@codemirror/commands'
import { markdownKeymap } from '@codemirror/lang-markdown'
import { closeSearchPanel, getSearchQuery, openSearchPanel, search, searchPanelOpen, setSearchQuery } from '@codemirror/search'
import { indentUnit, syntaxTree } from '@codemirror/language'
import type { Locale } from '@shared/types'
import { autoPair, countWords, minimalChange } from '../actions/markdown-ops'
import { focusTableCell, livePreview, liveMarkdown, startCodeBlock, leaveCodeBlock, deleteHorizontalRule } from './live-preview'
import { PageMargins } from './PageMargins'
import { searchChinese, searchCommands, previewSearchScroll, type SearchCommand } from './search'
import './editor.css'

export interface EditorHandle {
  getSource: () => string
  getCursor: () => number
  getSelection: () => { start: number; end: number } | null
  setSource: (text: string) => void
  replace: (start: number, end: number, text: string, cursor?: number) => void
  select: (start: number, end: number) => void
  jump: (offset: number) => void
  focus: () => void
  undo: () => void
  redo: () => void
  search: (command: SearchCommand) => void
  searchOpen: () => boolean
  pickCodeLanguage: () => void
  element: () => HTMLElement | null
}

interface Props {
  language: Locale
  source: string
  docPath: string
  onChange: (next: string) => void
  onContextMenu: (x: number, y: number) => void
  onOpenLink: (href: string) => void
  onCursorChange?: (cursor: number) => void
  onPaste?: (e: ClipboardEvent) => void
  readOnly?: boolean
  typewriter?: boolean
  highlightLine?: boolean
  spellCheck?: boolean
  autoPair?: boolean
  smartLists?: boolean
  tabSize?: number
  sourceMode?: boolean
}

export const Editor = forwardRef<EditorHandle, Props>(function Editor(props, ref) {
  const host = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const current = useRef(props)
  current.current = props
  const config = useRef(new Compartment())
  const documentConfig = useRef(new Compartment())
  const configuredDocument = useRef({ path: props.docPath, language: props.language, sourceMode: props.sourceMode })
  const [range, setRange] = useState({ from: 0, to: 0 })
  const preview = () => current.current.sourceMode ? [] : livePreview(current.current.docPath ? current.current.docPath.replace(/[\\/][^\\/]+$/, '') : null)

  const configure = () => [
    current.current.language === 'zh-CN' ? searchChinese : [],
    placeholder(t('从这里开始写作…')),
    EditorState.tabSize.of(current.current.tabSize ?? 2),
    indentUnit.of(' '.repeat(current.current.tabSize ?? 2)),
    EditorState.readOnly.of(!!current.current.readOnly),
    EditorView.editable.of(!current.current.readOnly),
    EditorView.contentAttributes.of({
      class: current.current.sourceMode ? 'md source-mode' : 'md',
      'aria-label': t('Markdown 文档'),
      spellcheck: String(!!current.current.spellCheck)
    }),
    current.current.highlightLine ? highlightActiveLine() : [],
    current.current.smartLists ? keymap.of(markdownKeymap) : []
  ]

  const select = (start: number, end: number): void => {
    const view = viewRef.current
    if (!view) return
    start = Math.min(start, view.state.doc.length)
    end = Math.min(end, view.state.doc.length)
    if (focusTableCell(view, start, end)) return
    view.dispatch({ selection: { anchor: start, head: end }, scrollIntoView: true })
    view.focus()
  }

  useImperativeHandle(ref, () => ({
    getSource: () => viewRef.current?.state.doc.toString() ?? current.current.source,
    getCursor: () => viewRef.current?.state.selection.main.from ?? 0,
    getSelection: () => {
      const range = viewRef.current?.state.selection.main
      return range && !range.empty ? { start: range.from, end: range.to } : null
    },
    setSource: (text) => {
      const view = viewRef.current
      // A minimal change keeps the caret and scroll position where the text did not move.
      if (view && !view.state.readOnly) view.dispatch({ changes: minimalChange(view.state.doc.toString(), text), annotations: isolateHistory.of('full') })
    },
    replace: (start, end, text, at) => {
      const view = viewRef.current
      if (!view || view.state.readOnly) return
      const cursor = at ?? start + text.length
      const change = minimalChange(view.state.doc.sliceString(start, end), text)
      view.dispatch({ changes: { ...change, from: start + change.from, to: start + change.to }, selection: { anchor: cursor }, userEvent: 'input' })
      select(cursor, cursor)
    },
    select,
    jump: (offset) => {
      const view = viewRef.current
      if (!view) return
      view.dispatch({ selection: { anchor: offset }, effects: EditorView.scrollIntoView(offset, { y: 'start', yMargin: 16 }) })
      view.focus()
    },
    focus: () => viewRef.current?.focus(),
    undo: () => { if (viewRef.current && !viewRef.current.state.readOnly) undo(viewRef.current) },
    redo: () => { if (viewRef.current && !viewRef.current.state.readOnly) redo(viewRef.current) },
    search: (command) => { if (viewRef.current) searchCommands[command](viewRef.current) },
    searchOpen: () => !!viewRef.current && searchPanelOpen(viewRef.current.state),
    pickCodeLanguage: () => {
      const view = viewRef.current
      if (!view || view.state.readOnly) return
      const at = view.state.selection.main.head
      const select = [...view.dom.querySelectorAll<HTMLSelectElement>('.live-code-language')].find(
        el => at >= Number(el.dataset.from) && at <= Number(el.dataset.to)
      )
      select?.focus()
      select?.showPicker()
    },
    element: () => viewRef.current?.contentDOM ?? null
  }), [])

  useLayoutEffect(() => {
    const view = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: current.current.source,
        extensions: [
          liveMarkdown(),
          history(),
          search({ top: true }),
          previewSearchScroll,
          config.current.of(configure()),
          keymap.of([{ key: 'Enter', run: startCodeBlock }, { key: 'Backspace', run: deleteHorizontalRule }, ...historyKeymap, indentWithTab, ...defaultKeymap]),
          EditorView.lineWrapping,
          scrollPastEnd(),
          documentConfig.current.of(preview()),
          EditorView.domEventHandlers({
            mousedown(event, view) {
              if (event.button !== 0 || view.state.readOnly) return false
              const footer = (event.target as HTMLElement).closest<HTMLElement>('.live-code-end')
              if (footer) {
                event.preventDefault()
                leaveCodeBlock(view, Number(footer.dataset.codeEnd))
                return true
              }
              const last = view.dom.querySelector<HTMLElement>(`.live-code-end[data-code-end="${view.state.doc.length}"]`)
              if (last && event.clientY > last.getBoundingClientRect().bottom) {
                event.preventDefault()
                leaveCodeBlock(view, view.state.doc.length)
                return true
              }
              return false
            },
            contextmenu(event) {
              event.preventDefault()
              current.current.onContextMenu(event.clientX, event.clientY)
              return true
            },
            paste(event) {
              if (current.current.readOnly) return true
              current.current.onPaste?.(event)
              return event.defaultPrevented
            }
          }),
          EditorView.inputHandler.of((view, from, to, text) => {
            if (!current.current.autoPair || view.composing || text.length !== 1) return false
            for (let node = syntaxTree(view.state).resolveInner(from, -1); node; node = node.parent!) {
              if (node.name === 'FencedCode' || node.name === 'CodeBlock' || node.name === 'InlineCode') return false
            }
            const source = view.state.doc.toString()
            const result = autoPair(source, from, from === to ? null : { start: from, end: to }, text)
            if (!result) return false
            if (result === 'skip-close') view.dispatch({ selection: { anchor: from + 1 } })
            else view.dispatch({
              changes: minimalChange(source, result.text),
              selection: { anchor: result.selection?.[0] ?? result.cursor, head: result.selection?.[1] ?? result.cursor },
              userEvent: 'input.type', scrollIntoView: true
            })
            return true
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !update.transactions.every((tr) => tr.annotation(Transaction.addToHistory) === false)) {
              current.current.onChange(update.state.doc.toString())
            }
            if (update.docChanged || update.selectionSet) {
              const offset = update.state.selection.main.from
              setRange({ from: offset, to: update.state.selection.main.to })
              current.current.onCursorChange?.(offset)
              if (current.current.typewriter && update.view.hasFocus && update.docChanged) {
                requestAnimationFrame(() => {
                  if (viewRef.current === view) view.dispatch({ effects: EditorView.scrollIntoView(offset, { y: 'center' }) })
                })
              }
            }
          })
        ]
      })
    })
    viewRef.current = view
    // Capture before widgets turn a table cell into an input or focus an image.
    const followLink = (event: MouseEvent): void => {
      const link = (event.target as HTMLElement).closest<HTMLElement>('[data-md-href], a[href]')
      if (!link || event.button !== 0) return
      if (event.type === 'click') event.preventDefault()
      if (!event.ctrlKey && !event.metaKey) return
      event.preventDefault()
      event.stopPropagation()
      if (event.type === 'click') current.current.onOpenLink(link.dataset.mdHref ?? link.getAttribute('href')!)
    }
    view.dom.addEventListener('mousedown', followLink, true)
    view.dom.addEventListener('click', followLink, true)
    view.focus()
    return () => {
      viewRef.current = null
      view.dom.removeEventListener('mousedown', followLink, true)
      view.dom.removeEventListener('click', followLink, true)
      view.destroy()
    }
  }, [])

  useEffect(() => {
    const { path, language, sourceMode } = configuredDocument.current
    if (path === props.docPath && language === props.language && sourceMode === props.sourceMode) return
    configuredDocument.current = { path: props.docPath, language: props.language, sourceMode: props.sourceMode }
    viewRef.current?.dispatch({ effects: documentConfig.current.reconfigure(preview()) })
  }, [props.docPath, props.language, props.sourceMode])

  useEffect(() => {
    viewRef.current?.dispatch({ effects: config.current.reconfigure(configure()) })
  }, [props.readOnly, props.highlightLine, props.spellCheck, props.smartLists, props.tabSize, props.language, props.sourceMode])

  useEffect(() => {
    const view = viewRef.current
    if (view && searchPanelOpen(view.state)) {
      const query = getSearchQuery(view.state)
      closeSearchPanel(view)
      openSearchPanel(view)
      view.dispatch({ effects: setSearchQuery.of(query) })
    }
  }, [props.readOnly, props.language])

  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    const text = view.state.toText(props.source)
    if (text.eq(view.state.doc)) return
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
      annotations: Transaction.addToHistory.of(false)
    })
  }, [props.source])

  const before = props.source.slice(0, range.from)
  const words = useMemo(() => countWords(props.source), [props.source])
  return (
    <div className="editor">
      <div className="editor__surface">
        <div className="editor__mount" ref={host} />
        <PageMargins />
      </div>
      <div className="editor__status">
        <span>{t('第 {0} 行', before.split('\n').length)}</span>
        <span>{range.to > range.from ? t('已选 {0} / 共 {1} 字', countWords(props.source.slice(range.from, range.to)), words) : t('{0} 字', words)}</span>
        <span>{props.readOnly ? t('只读') : props.sourceMode ? t('源码模式') : t('直接编辑')}</span>
      </div>
    </div>
  )
})
