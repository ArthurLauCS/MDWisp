import { t } from '../i18n'
import { StateField, type EditorState, type Range } from '@codemirror/state'
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet } from '@codemirror/view'
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language'
import type { SyntaxNode } from '@lezer/common'
import { getSearchQuery, searchPanelOpen } from '@codemirror/search'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import hljs from 'highlight.js/lib/common'
import { detectLanguage } from '@shared/markdown/detect'
import { frontmatterEnd } from '@shared/markdown/frontmatter'
import { markdownReferences, type MarkdownReferences } from '@shared/markdown/pipeline'
import { blockMath, findInlineMath, renderMath } from '@shared/markdown/math'
import { renderMarkdown } from '../markdown/render'
import { resolveDocumentAssets } from '../markdown/resolve-assets'
import { linkResolver } from '../markdown/links'
import { settingsSnapshot } from '../state/settings'
import { cellRange, escapePipes, findTableAt, parseTable, serializeTable, type TableContext } from '../table/model'

const highlightDelimiter = { resolve: 'Highlight', mark: 'HighlightMark' }
const documentCache = new WeakMap<EditorState['doc'], { source: string; references: MarkdownReferences; referenceKey: string }>()

function documentData(state: EditorState) {
  let data = documentCache.get(state.doc)
  if (!data) {
    const source = state.doc.toString()
    const references = markdownReferences(source)
    data = { source, references, referenceKey: JSON.stringify(references) }
    documentCache.set(state.doc, data)
  }
  return data
}

export function startCodeBlock(view: EditorView): boolean {
  const { state } = view
  const cursor = state.selection.main
  if (state.readOnly || !cursor.empty) return false
  const row = state.doc.lineAt(cursor.head)
  const fence = /^( {0,3})(`{3,}|~{3,})[\w+-]*\s*$/.exec(row.text)
  const node = syntaxTree(state).resolveInner(row.from + row.text.length, -1)
  let block = node
  while (block.parent && block.name !== 'FencedCode') block = block.parent
  if (!fence || cursor.head !== row.to || block.name !== 'FencedCode' || block.from !== row.from + fence[1].length) return false
  if (block.getChildren('CodeMark').length > 1) return false
  view.dispatch({ changes: { from: row.to, insert: `\n${fence[1]}\n${fence[1]}${fence[2]}` }, selection: { anchor: row.to + 1 + fence[1].length }, userEvent: 'input.type' })
  return true
}

export function deleteHorizontalRule(view: EditorView): boolean {
  const { state } = view
  const cursor = state.selection.main
  if (state.readOnly || !cursor.empty || state.selection.ranges.length !== 1) return false
  let row = state.doc.lineAt(cursor.head)
  if (cursor.head === row.from && row.number > 1) row = state.doc.line(row.number - 1)
  const tree = ensureSyntaxTree(state, row.to, 100)
  if (!tree || row.from < frontmatterEnd(state.doc.toString())) return false
  let rule = false
  tree.iterate({ from: row.from, to: row.to, enter(node) {
    if (node.name === 'HorizontalRule' && node.from >= row.from && node.to <= row.to) rule = true
  } })
  if (!rule) return false
  const from = row.number === state.doc.lines && row.from > 0 ? row.from - 1 : row.from
  const to = row.number < state.doc.lines ? row.to + 1 : row.to
  view.dispatch({ changes: { from, to }, selection: { anchor: from }, scrollIntoView: true, userEvent: 'delete.backward' })
  return true
}

export function leaveCodeBlock(view: EditorView, end: number): void {
  if (view.state.readOnly) return
  const doc = view.state.doc
  const next = end < doc.length ? doc.lineAt(end + 1) : null
  const at = next?.from ?? end
  const insert = next ? (next.length ? '\n' : '') : '\n\n'
  view.dispatch({ changes: { from: at, insert }, selection: { anchor: next ? at : at + insert.length }, scrollIntoView: true, userEvent: 'input' })
  view.focus()
}

class CodeLanguage extends WidgetType {
  readonly language = settingsSnapshot().language
  constructor(readonly from: number, readonly infoFrom: number, readonly infoTo: number, readonly to: number,
    readonly lang: string, readonly detected: string | null, readonly readOnly: boolean) { super() }
  eq(other: CodeLanguage): boolean {
    return this.language === other.language && this.from === other.from && this.infoFrom === other.infoFrom && this.infoTo === other.infoTo && this.to === other.to &&
      this.lang === other.lang && this.detected === other.detected && this.readOnly === other.readOnly
  }
  toDOM(view: EditorView): HTMLElement {
    const select = document.createElement('select')
    select.className = 'live-code-language'
    select.setAttribute('aria-label', t('代码语言'))
    select.dataset.from = String(this.from)
    select.dataset.to = String(this.to)
    select.disabled = this.readOnly
    select.add(new Option(this.detected ? t('自动检测 · {0}', this.detected) : t('自动检测'), ''))
    select.add(new Option(t('纯文本'), 'text'))
    const languages = hljs.listLanguages()
    if (this.lang && this.lang !== 'text' && !languages.includes(this.lang)) languages.push(this.lang)
    for (const lang of languages.sort()) select.add(new Option(lang, lang))
    select.value = this.lang
    select.onchange = () => {
      view.dispatch({ changes: { from: this.infoFrom, to: this.infoTo, insert: select.value }, userEvent: 'input' })
      view.focus()
    }
    return select
  }
}

export function liveMarkdown() {
  return markdown({ base: markdownLanguage, addKeymap: false, extensions: [{
    defineNodes: ['Highlight', 'HighlightMark'],
    parseInline: [{ name: 'Highlight', parse(cx, next, pos) {
      if (next !== 61 || cx.char(pos + 1) !== 61) return -1
      return cx.addDelimiter(highlightDelimiter, pos, pos + 2,
        !/\s/.test(cx.slice(pos + 2, pos + 3)), !/\s/.test(cx.slice(pos - 1, pos)))
    } }]
  }] })
}

class TaskCheckbox extends WidgetType {
  readonly language = settingsSnapshot().language
  constructor(readonly from: number, readonly checked: boolean, readonly readOnly: boolean) { super() }
  eq(other: TaskCheckbox): boolean {
    return this.language === other.language && this.from === other.from && this.checked === other.checked && this.readOnly === other.readOnly
  }
  toDOM(view: EditorView): HTMLElement {
    const input = document.createElement('input')
    input.type = 'checkbox'
    input.checked = this.checked
    input.disabled = this.readOnly
    input.setAttribute('aria-label', t('完成任务'))
    input.onchange = () => view.dispatch({ changes: { from: this.from + 1, to: this.from + 2, insert: input.checked ? 'x' : ' ' }, userEvent: 'input' })
    return input
  }
}

class Rendered extends WidgetType {
  readonly language = settingsSnapshot().language
  constructor(readonly html: string, readonly className = '') { super() }
  eq(other: Rendered): boolean { return this.language === other.language && this.html === other.html && this.className === other.className }
  toDOM(): HTMLElement {
    const dom = document.createElement('span')
    dom.className = `live-rendered ${this.className}`
    dom.innerHTML = this.html
    for (const image of dom.querySelectorAll('img')) {
      image.tabIndex = 0
      image.title = t('点击选中图片，按 Z 全屏查看，Esc 返回')
      image.onmousedown = event => {
        event.preventDefault()
        event.stopPropagation()
        image.focus()
      }
    }
    return dom
  }
  ignoreEvent(): boolean { return false }
}

const tables = new WeakMap<HTMLElement, LiveTable>()

interface PreviewMatch { from: number; to: number; selected: boolean }

function matchClass(matches: PreviewMatch[], from: number, to: number): string {
  let lo = 0, hi = matches.length
  while (lo < hi) {
    const mid = (lo + hi) >>> 1
    if (matches[mid].to <= from) lo = mid + 1
    else hi = mid
  }
  let className = ''
  for (let i = lo; i < matches.length && matches[i].from < to; i++) {
    if (matches[i].selected) return 'cm-searchMatch cm-searchMatch-selected'
    className = 'cm-searchMatch'
  }
  return className
}

function cellHtml(raw: string, docDir: string | null, references: MarkdownReferences): string {
  const dom = document.createElement('div')
  dom.innerHTML = resolveDocumentAssets(renderMarkdown(raw, references), docDir)
  return dom.firstElementChild?.tagName === 'P' ? dom.firstElementChild.innerHTML : dom.innerHTML
}

/** Keep a cell's input alive through transactions, including IME composition. */
class LiveTable extends WidgetType {
  readonly language = settingsSnapshot().language
  constructor(readonly ctx: TableContext, readonly docDir: string | null, readonly readOnly: boolean,
    readonly references: MarkdownReferences, readonly referenceKey: string, readonly matches: PreviewMatch[]) {
    super()
  }
  eq(other: LiveTable): boolean {
    return this.referenceKey === other.referenceKey && this.language === other.language && this.docDir === other.docDir && this.ctx.raw === other.ctx.raw && this.ctx.start === other.ctx.start && this.readOnly === other.readOnly && JSON.stringify(this.matches) === JSON.stringify(other.matches)
  }
  toDOM(view: EditorView): HTMLElement {
    const dom = document.createElement('div')
    dom.className = 'live-table'
    this.paint(dom)
    dom.addEventListener('mousedown', (event) => {
      const target = event.target as HTMLElement
      if (target.matches('.live-cell-input')) return
      const cell = target.closest<HTMLElement>('[data-cell-from]')
      if (!cell || view.state.readOnly || event.button !== 0) return
      // Measure the editable text with the browser, including font metrics
      // and column alignment, instead of guessing a character width.
      const text = document.createElement('span')
      text.className = 'live-cell-input'
      text.textContent = cell.dataset.raw ?? ''
      cell.replaceChildren(text)
      const caret = document.caretRangeFromPoint(event.clientX, event.clientY)
      const at = caret?.startContainer === text.firstChild ? caret.startOffset : text.textContent.length
      event.preventDefault()
      editCell(view, cell, at)
    })
    dom.addEventListener('click', (event) => {
      const cell = (event.target as HTMLElement).closest<HTMLElement>('[data-cell-from]')
      if (cell && !cell.querySelector('textarea') && !view.state.readOnly) editCell(view, cell)
    })
    dom.addEventListener('input', (event) => {
      const input = event.target as HTMLTextAreaElement
      if (!input.matches('.live-cell-input') || view.state.readOnly) return
      const cell = input.parentElement!
      const current = tables.get(dom)!
      const position = { row: Number(cell.dataset.row), col: Number(cell.dataset.col) }
      const range = cellRange(current.ctx, position)
      const insert = escapePipes(input.value).replace(/\r?\n/g, '<br>')
      if (range.missing) {
        const cells = parseTable(current.ctx)
        cells[position.row][position.col] = insert
        const raw = serializeTable(cells, current.ctx.aligns)
        const next = { ...current.ctx, raw, end: current.ctx.start + raw.length }
        view.dispatch({ changes: { from: current.ctx.start, to: current.ctx.end, insert: raw }, selection: { anchor: cellRange(next, position).to }, userEvent: 'input.type' })
        return
      }
      const cursor = range.from + Math.min(input.selectionStart ?? insert.length, insert.length)
      view.dispatch({ changes: { ...range, insert }, selection: { anchor: cursor }, userEvent: 'input.type' })
    })
    dom.addEventListener('focusout', () => {
      // Wait for a toolbar click to capture the current table context first.
      requestAnimationFrame(() => {
        if (!dom.contains(document.activeElement)) tables.get(dom)?.paint(dom)
      })
    })
    return dom
  }
  paint(dom: HTMLElement): void {
    tables.set(dom, this)
    dom.innerHTML = resolveDocumentAssets(renderMarkdown(this.ctx.raw, this.references), this.docDir)
    const rows = parseTable(this.ctx)
    dom.querySelectorAll('tr').forEach((tr, row) => {
      tr.querySelectorAll<HTMLElement>('th, td').forEach((cell, col) => {
        const range = cellRange(this.ctx, { row, col })
        cell.dataset.row = String(row)
        cell.dataset.col = String(col)
        cell.dataset.cellFrom = String(range.from)
        cell.dataset.cellTo = String(range.to)
        cell.dataset.raw = rows[row][col]
        cell.className = matchClass(this.matches, range.from, range.to)
        cell.tabIndex = this.readOnly ? -1 : 0
        cell.setAttribute('aria-label', t('第 {0} 行，第 {1} 列', row + 1, col + 1))
        cell.onfocus = () => {
          if (!this.readOnly && !cell.querySelector('textarea')) editCell(EditorView.findFromDOM(dom)!, cell)
        }
      })
    })
  }
  updateDOM(dom: HTMLElement): boolean {
    const previous = tables.get(dom)!
    const input = dom.querySelector<HTMLTextAreaElement>('.live-cell-input')
    const sameShape = previous.ctx.cols === this.ctx.cols && previous.ctx.bodyRows === this.ctx.bodyRows
    tables.set(dom, this)
    if (input && document.activeElement === input && sameShape && !this.readOnly) {
      dom.querySelectorAll<HTMLElement>('[data-cell-from]').forEach((cell) => {
        const range = cellRange(this.ctx, { row: Number(cell.dataset.row), col: Number(cell.dataset.col) })
        const raw = this.ctx.raw.slice(range.from - this.ctx.start, range.to - this.ctx.start)
        if (!cell.contains(input) && (cell.dataset.raw !== raw || previous.referenceKey !== this.referenceKey)) cell.innerHTML = cellHtml(raw, this.docDir, this.references)
        cell.dataset.cellFrom = String(range.from)
        cell.dataset.cellTo = String(range.to)
        cell.dataset.raw = raw
        cell.className = matchClass(this.matches, range.from, range.to)
        cell.setAttribute('aria-label', t('第 {0} 行，第 {1} 列', Number(cell.dataset.row) + 1, Number(cell.dataset.col) + 1))
        if (cell.contains(input) && input.value !== cell.dataset.raw) input.value = cell.dataset.raw
      })
    } else {
      this.paint(dom)
    }
    return true
  }
  ignoreEvent(): boolean { return true }
}

function editCell(view: EditorView, cell: HTMLElement, start?: number, end = start): void {
  const table = cell.closest<HTMLElement>('.live-table')!
  for (const other of table.querySelectorAll<HTMLTextAreaElement>('.live-cell-input')) {
    if (other.parentElement !== cell) {
      const previous = other.parentElement!
      previous.innerHTML = cellHtml(previous.dataset.raw ?? '', tables.get(table)!.docDir, tables.get(table)!.references)
    }
  }
  let input = cell.querySelector('textarea')
  if (!input) {
    input = document.createElement('textarea')
    input.className = 'live-cell-input'
    input.value = cell.dataset.raw ?? ''
    input.setAttribute('aria-label', cell.getAttribute('aria-label')!)
    cell.replaceChildren(input)
    const sync = (): void => {
      if (!input!.isConnected) return
      const from = Number(cell.dataset.cellFrom)
      view.dispatch({ selection: { anchor: from + (input!.selectionStart ?? 0), head: from + (input!.selectionEnd ?? 0) } })
    }
    input.addEventListener('select', sync)
    input.addEventListener('keyup', sync)
    input.addEventListener('click', sync)
    // Enter moves between rows like a spreadsheet; line breaks go through the Alt+Enter action.
    input.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' || event.isComposing) return
      event.preventDefault()
      const row = Number(cell.dataset.row) + (event.shiftKey ? -1 : 1)
      const next = table.querySelector<HTMLElement>(`[data-row="${row}"][data-col="${cell.dataset.col}"]`)
      if (next) editCell(view, next)
    })
  }
  input.focus()
  input.setSelectionRange(start ?? input.value.length, end ?? input.value.length)
  const from = Number(cell.dataset.cellFrom)
  view.dispatch({ selection: { anchor: from + (input.selectionStart ?? 0), head: from + (input.selectionEnd ?? 0) } })
}

export function focusTableCell(view: EditorView, start: number, end: number): boolean {
  if (view.state.readOnly) return false
  const cell = [...view.dom.querySelectorAll<HTMLElement>('[data-cell-from]')].find(
    (el) => start >= Number(el.dataset.cellFrom) && end <= Number(el.dataset.cellTo)
  )
  if (!cell) return false
  editCell(view, cell, start - Number(cell.dataset.cellFrom), end - Number(cell.dataset.cellFrom))
  return true
}

export function previewDecorations(state: EditorState, docDir: string | null): DecorationSet {
  const searching = searchPanelOpen(state)
  const matches: PreviewMatch[] = []
  const query = getSearchQuery(state)
  if (searching && query.valid) {
    const cursor = query.getCursor(state)
    for (let next = cursor.next(); !next.done; next = cursor.next()) {
      const { from, to } = next.value
      matches.push({ from, to, selected: state.selection.ranges.some(range => range.from === from && range.to === to) })
    }
  }
  const ranges: Range<Decoration>[] = []
  const listLines = new Map<number, { depth: number; first: boolean }>()
  const codeLines = new Set<number>()
  const { source, references, referenceKey } = documentData(state)
  const resolveLink = linkResolver(source, references)
  const hide = (from: number, to: number): void => {
    if (to > from) {
      ranges.push(Decoration.replace({}).range(from, to))
      const match = matchClass(matches, from, to)
      if (match) line(from, match)
    }
  }
  const mark = (from: number, to: number, className: string): void => {
    ranges.push(Decoration.mark({ class: className }).range(from, to))
  }
  const line = (at: number, className: string): void => {
    ranges.push(Decoration.line({ class: className }).range(state.doc.lineAt(at).from))
  }
  const body = frontmatterEnd(source)
  for (let pos = 0; pos < body; pos = state.doc.lineAt(pos).to + 1) line(pos, 'live-frontmatter')
  const tree = syntaxTree(state)
  const maths: { from: number; to: number }[] = []
  const editing = (from: number, to: number): boolean => searching || state.selection.ranges.some(r => r.from <= to && r.to >= from)
  const inCode = (at: number): boolean => {
    for (let node: SyntaxNode | null = tree.resolveInner(at, 1); node; node = node.parent) {
      if (['InlineCode', 'Autolink', 'URL', 'HTMLTag'].includes(node.name)) return true
    }
    return false
  }
  tree.iterate({
    enter(node) {
      const { name, from, to } = node
      if (from < body && name !== 'Document') return false
      // Markdown parsed inside a formula (such as `_` emphasis) is not markup.
      if (maths.some(m => from >= m.from && to <= m.to)) return false
      const heading = /^(?:ATX|Setext)Heading(\d)$/.exec(name)
      if (heading) line(from, `live-heading live-h${heading[1]}`)
      if (name === 'Paragraph' || heading) {
        const tex = heading ? null : blockMath(source.slice(from, to))
        // A block widget must cover whole lines, so list or quote prefixes keep the source.
        if (tex !== null && state.doc.lineAt(from).from === from) {
          const widget = new Rendered(renderMath(tex, true), 'live-math-block')
          maths.push({ from, to })
          if (editing(from, to)) {
            mark(from, to, 'live-math-source')
            ranges.push(Decoration.widget({ widget, block: true, side: 1 }).range(to))
          } else ranges.push(Decoration.replace({ widget, block: true }).range(from, to))
          return false
        }
        for (const math of findInlineMath(source.slice(from, to), at => inCode(from + at))) {
          const a = from + math.from, b = from + math.to
          maths.push({ from: a, to: b })
          if (editing(a, b)) mark(a, b, 'live-math-source')
          else ranges.push(Decoration.replace({ widget: new Rendered(renderMath(math.tex, false), 'live-math') }).range(a, b))
        }
      }
      if (name === 'Paragraph') {
        line(from, 'live-paragraph-start')
        line(to, 'live-paragraph-end')
      }
      if (name === 'ListItem') {
        let depth = 1
        for (let parent = node.node.parent; parent; parent = parent.parent) if (parent.name === 'ListItem') depth++
        for (let pos = state.doc.lineAt(from).from; pos < to; pos = state.doc.lineAt(pos).to + 1) {
          listLines.set(pos, { depth, first: pos === state.doc.lineAt(from).from })
        }
      }
      if (name === 'HeaderMark') {
        const end = source[to] === ' ' ? to + 1 : to
        hide(from, end)
      }
      if (name === 'StrongEmphasis') mark(from, to, 'live-strong')
      if (name === 'Emphasis') mark(from, to, 'live-em')
      if (name === 'Strikethrough') mark(from, to, 'live-strike')
      if (name === 'InlineCode') {
        mark(from, to, 'live-code')
        const delimiters = node.node.getChildren('CodeMark')
        if (delimiters.length === 2) {
          const inner = source.slice(delimiters[0].to, delimiters[1].from)
          if (inner.startsWith(' ') && inner.endsWith(' ') && inner.trim()) {
            hide(delimiters[0].to, delimiters[0].to + 1)
            hide(delimiters[1].from - 1, delimiters[1].from)
          }
        }
      }
      if (name === 'Highlight') mark(from, to, 'live-mark')
      if (name === 'Superscript') mark(from, to, 'live-sup')
      if (name === 'Subscript') mark(from, to, 'live-sub')
      if (['EmphasisMark', 'StrikethroughMark', 'CodeMark', 'HighlightMark', 'SuperscriptMark', 'SubscriptMark'].includes(name)) hide(from, to)
      if (name === 'TaskMarker') {
        ranges.push(Decoration.replace({ widget: new TaskCheckbox(from, /x/i.test(source.slice(from, to)), state.readOnly) }).range(from, to))
        return false
      }
      if (name === 'HTMLTag' && /^<(u|strong|em|del|mark)>$/.test(source.slice(from, to))) {
        const tag = source.slice(from + 1, to - 1)
        // Match parsed sibling tags so code samples and nested tags cannot close this span.
        let depth = 1, closing = node.node.nextSibling
        for (; closing; closing = closing.nextSibling) {
          if (closing.name !== 'HTMLTag') continue
          const raw = source.slice(closing.from, closing.to)
          if (raw === `<${tag}>`) depth++
          if (raw === `</${tag}>` && --depth === 0) break
        }
        if (closing && closing.from > to) {
          hide(from, to)
          mark(to, closing.from, ({ u: 'live-underline', strong: 'live-strong', em: 'live-em', del: 'live-strike', mark: 'live-mark' } as Record<string, string>)[tag])
          hide(closing.from, closing.to)
        }
      }
      if (name === 'QuoteMark') {
        line(from, 'live-quote')
        hide(from, source[to] === ' ' ? to + 1 : to)
      }
      if (name === 'ListMark') {
        if (/[-+*]/.test(source.slice(from, to))) {
          ranges.push(Decoration.replace({ widget: new Rendered('<span class="live-bullet">•</span>', 'live-list-marker') }).range(from, to + (source[to] === ' ' ? 1 : 0)))
        } else mark(from, to + (source[to] === ' ' ? 1 : 0), 'live-list-marker')
      }
      if (name === 'Link' || name === 'Autolink' || (name === 'URL' && !['Link', 'Autolink', 'Image', 'LinkReference'].includes(node.node.parent?.name ?? ''))) {
        const href = resolveLink(node.node)
        if (href === null) return false
        ranges.push(Decoration.mark({ class: `live-link ${matchClass(matches, from, to)}`, attributes: { 'data-md-href': href, title: t('Ctrl+点击打开链接') } }).range(from, to))
        // The label stays editable; reveal the destination only while editing it.
        const end = node.node.getChildren('LinkMark')[1]
        if (end) {
          hide(from, from + 1)
          if (searching || state.selection.main.head <= end.from || state.selection.main.head >= to) hide(end.from, to)
        }
        if (name !== 'Link') return false
      }
      if (name === 'Image') {
        const container = node.node.parent?.name === 'Link' ? node.node.parent : node.node
        const row = state.doc.lineAt(from)
        const standalone = !source.slice(row.from, container.from).trim() && !source.slice(container.to, row.to).trim()
        if (standalone) line(from, 'live-image-line')
        const selected = state.selection.ranges.some((r) => r.from < to && r.to > from)
        const inside = state.selection.main.head > from && state.selection.main.head < to
        if (searching || standalone || (!selected && !inside)) {
          const html = resolveDocumentAssets(renderMarkdown(source.slice(from, to), references), docDir)
          ranges.push(Decoration.replace({ widget: new Rendered(html, matchClass(matches, from, to)) }).range(from, to))
        }
        return false
      }
      if (name === 'HorizontalRule') {
        line(from, `live-rule ${matchClass(matches, from, to)}`)
        ranges.push(Decoration.replace({ widget: new Rendered('<hr>') }).range(from, to))
        return false
      }
      if (name === 'FencedCode') {
        const first = state.doc.lineAt(from)
        const marks = node.node.getChildren('CodeMark')
        const closing = marks[1]
        const code = node.node.getChild('CodeText')
        const lang = node.node.getChild('CodeInfo')
        // Keep an unfinished opening fence visible until Enter creates a body.
        if (first.to >= to) return false
        const focused = state.selection.main.head >= from && state.selection.main.head <= to
        for (let pos = first.from; pos <= to;) {
          const row = state.doc.lineAt(pos)
          codeLines.add(row.from)
          const header = pos === first.from
          const footer = !!closing && row.from === state.doc.lineAt(closing.from).from
          ranges.push(Decoration.line({
            class: `live-code-line${header ? ` live-code-header ${matchClass(matches, from, first.to)}` : ''}${footer ? ' live-code-end' : ''}${focused && header ? ' is-active' : ''}`,
            attributes: { 'data-code-from': String(from), ...(footer ? { 'data-code-end': String(to) } : {}) }
          }).range(row.from))
          pos = row.to + 1
        }
        ranges.push(Decoration.replace({ widget: new CodeLanguage(from, marks[0].to, first.to, to,
          lang ? source.slice(lang.from, lang.to) : '', code ? detectLanguage(source.slice(code.from, code.to)).lang : null, state.readOnly)
        }).range(from, first.to))
        if (closing) hide(closing.from, closing.to)
        return false
      }
      if (name === 'CodeBlock') {
        for (let pos = from; pos <= to;) {
          const row = state.doc.lineAt(pos)
          codeLines.add(row.from)
          line(pos, 'live-code-line')
          pos = row.to + 1
        }
      }
      if (name === 'Table') {
        const ctx = findTableAt(source, from)
        if (ctx) ranges.push(Decoration.replace({ widget: new LiveTable(ctx, docDir, state.readOnly, references, referenceKey, matches.filter(match => match.from < ctx.end && match.to > ctx.start)), block: true }).range(ctx.start, ctx.end))
        return false
      }
    }
  })
  for (const [from, { depth, first }] of listLines) {
    if (codeLines.has(from)) continue
    ranges.push(Decoration.line({ class: `live-list${first ? ' live-list-start' : ''}`, attributes: { style: `--list-depth: ${depth}` } }).range(from))
    hide(from, from + /^\s*/.exec(state.doc.lineAt(from).text)![0].length)
  }
  for (let number = 1; number <= state.doc.lines; number++) {
    const row = state.doc.line(number)
    if (row.from >= body && !row.text.trim() && !codeLines.has(row.from)) line(row.from, 'live-blank')
  }
  return Decoration.set(ranges, true)
}

// Highlight the editable text with the existing renderer's tokens. These
// marks never replace code, so clicking and dragging keep native coordinates.
function codeHighlights(view: EditorView): DecorationSet {
  const { state, viewport } = view
  const ranges: Range<Decoration>[] = []
  syntaxTree(state).iterate({ from: viewport.from, to: viewport.to, enter(node) {
    if (node.name !== 'FencedCode') return
    const code = node.node.getChild('CodeText')
    if (!code) return false
    const dom = document.createElement('div')
    dom.innerHTML = renderMarkdown(state.doc.sliceString(node.from, node.to))
    const rendered = dom.querySelector('.codeblock__pre > code')
    // Blockquotes and list prefixes can make the rendered offsets differ.
    if (!rendered || rendered.textContent?.trimEnd() !== state.doc.sliceString(code.from, code.to).trimEnd()) return false
    let pos = code.from
    const walk = (parent: Node): void => {
      for (const child of parent.childNodes) {
        const from = pos
        if (child.nodeType === Node.TEXT_NODE) pos += child.textContent!.length
        else {
          walk(child)
          const className = (child as HTMLElement).className
          const to = Math.min(pos, code.to)
          if (className && to > from) ranges.push(Decoration.mark({ class: className }).range(from, to))
        }
      }
    }
    walk(rendered)
    return false
  } })
  return Decoration.set(ranges, true)
}

function selectionAffectsPreview(state: EditorState): boolean {
  if (searchPanelOpen(state) || state.selection.ranges.some(range => !range.empty)) return true
  for (const range of state.selection.ranges) {
    for (let node = syntaxTree(state).resolveInner(range.head, -1); node; node = node.parent!) {
      if (['Link', 'Image', 'FencedCode'].includes(node.name)) return true
      if ((node.name === 'Paragraph' || node.name.includes('Heading')) && state.doc.sliceString(node.from, node.to).includes('$')) return true
    }
  }
  return false
}

export function livePreview(docDir: string | null) {
  return [EditorView.domEventHandlers({
    mouseover(event, view) {
      const line = (event.target as HTMLElement).closest<HTMLElement>('[data-code-from]')
      for (const header of view.dom.querySelectorAll<HTMLElement>('.live-code-header')) {
        header.classList.toggle('is-hovered', !!line && header.dataset.codeFrom === line.dataset.codeFrom)
      }
    },
    mouseleave(_event, view) {
      for (const header of view.dom.querySelectorAll('.live-code-header.is-hovered')) header.classList.remove('is-hovered')
    }
  }), StateField.define<DecorationSet>({
    create: (state) => previewDecorations(state, docDir),
    update: (value, tr) => tr.docChanged || (tr.selection && (selectionAffectsPreview(tr.startState) || selectionAffectsPreview(tr.state))) || tr.reconfigured || getSearchQuery(tr.state) !== getSearchQuery(tr.startState) || searchPanelOpen(tr.state) !== searchPanelOpen(tr.startState) || syntaxTree(tr.state) !== syntaxTree(tr.startState)
      ? previewDecorations(tr.state, docDir) : value,
    provide: (field) => EditorView.decorations.from(field)
  }), ViewPlugin.define((view) => ({
    decorations: codeHighlights(view),
    update(update) {
      if (update.docChanged || update.viewportChanged || syntaxTree(update.state) !== syntaxTree(update.startState)) this.decorations = codeHighlights(update.view)
    }
  }), { decorations: (plugin) => plugin.decorations })]
}
