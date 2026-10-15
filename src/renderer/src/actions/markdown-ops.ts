import { t } from '../i18n'
import { markdownLanguage } from '@codemirror/lang-markdown'
/**
 * Source-level markdown transforms. Every function is pure: it takes the
 * whole document and a selection, and returns the new document plus where
 * the cursor lands. Keeping them free of editor state makes them testable
 * and lets the same op serve a shortcut, a menu item and the palette.
 */

export interface EditResult {
  text: string
  cursor: number
  /** Selection to restore, when the op leaves something selected. */
  selection?: [number, number]
}

/** Preserve CodeMirror's unchanged ranges and scroll anchors. */
export function minimalChange(source: string, text: string) {
  let from = 0
  while (from < source.length && from < text.length && source[from] === text[from]) from++
  let to = source.length, end = text.length
  while (to > from && end > from && source[to - 1] === text[end - 1]) {
    to--
    end--
  }
  return { from, to, insert: text.slice(from, end) }
}

/** Expand a range to whole lines, returning the line boundaries. */
function lineBounds(src: string, start: number, end: number): [number, number] {
  const from = start === 0 ? 0 : src.lastIndexOf('\n', start - 1) + 1
  let to = src.indexOf('\n', end > start && src[end - 1] === '\n' ? end - 1 : end)
  if (to === -1) to = src.length
  return [from, to]
}

function isWrapped(text: string, open: string, close: string): boolean {
  if (open === '==') return !text.slice(open.length, -close.length).includes('==')
  const tree = markdownLanguage.parser.parse(text)
  const name = open.startsWith('`') ? 'InlineCode' : ({ '**': 'StrongEmphasis', '*': 'Emphasis', '~~': 'Strikethrough' } as Record<string, string>)[open]
  if (name) {
    let found = false
    tree.iterate({ enter(node) { if (node.name === name && node.from === 0 && node.to === text.length) found = true } })
    return found
  }
  let depth = 0, end = -1
  tree.iterate({ enter(node) {
    if (node.name !== 'HTMLTag') return
    const raw = text.slice(node.from, node.to)
    if (raw === open) depth++
    if (raw === close && --depth === 0 && end < 0) end = node.to
  } })
  return end === text.length
}

export function toggleInline(src: string, start: number, end: number, wrap: string): EditResult {
  const tag = ({ '**': 'strong', '*': 'em', '~~': 'del', '==': 'mark', '<u>': 'u' } as Record<string, string>)[wrap]
  const close = wrap === '<u>' ? '</u>' : wrap
  if (start === end) {
    // Nothing selected: insert the pair and put the cursor between them so
    // the next typed character lands inside the emphasis.
    const text = src.slice(0, start) + wrap + close + src.slice(end)
    return { text, cursor: start + wrap.length }
  }
  if (!src.slice(start, end).trim()) return { text: src, cursor: start, selection: [start, end] }
  if (src.slice(start, end).includes('\n')) {
    const selected = src.slice(start, end)
    let offset = start
    const text = selected.split('\n').map(row => {
      const result = row.trim() ? toggleInline(src, offset, offset + row.length, wrap).text : src
      const replacement = result.slice(offset, result.length - (src.length - offset - row.length))
      offset += row.length + 1
      return replacement
    }).join('\n')
    return { text: src.slice(0, start) + text + src.slice(end), cursor: start, selection: [start, start + text.length] }
  }
  // Spaces outside emphasis remain prose, rather than invalidating the delimiters.
  if (wrap !== '`') {
    const raw = src.slice(start, end)
    start += raw.length - raw.trimStart().length
    end -= raw.length - raw.trimEnd().length
  }
  const selected = src.slice(start, end)
  const pairs: [string, string][] = [[wrap, close]]
  if (tag && wrap !== '<u>') pairs.unshift([`<${tag}>`, `</${tag}>`])
  if (wrap === '`') {
    const ticks = /(`+)( ?)$/.exec(src.slice(0, start))
    if (ticks) pairs.unshift([ticks[1] + ticks[2], ticks[2] + ticks[1]])
    const selectedTicks = /^`+/.exec(selected)?.[0]
    if (selectedTicks) pairs.unshift([selectedTicks, selectedTicks])
  }
  for (const [open, shut] of pairs) {
    const before = src.slice(Math.max(0, start - open.length), start)
    const after = src.slice(end, end + shut.length)
    // One star out of ** is not an italic wrapper; *** contains both formats.
    const italicInBold = open === '*' && (src.slice(0, start).match(/\*+$/)?.[0].length ?? 0) % 2 === 0
    if (before === open && after === shut && !italicInBold && isWrapped(before + selected + after, open, shut)) {
      return { text: src.slice(0, start - open.length) + selected + src.slice(end + shut.length),
        cursor: start - open.length, selection: [start - open.length, end - open.length] }
    }
    const leading = /^\*+/.exec(selected)?.[0].length ?? 0
    if (selected.startsWith(open) && selected.endsWith(shut) && selected.length > open.length + shut.length &&
      !(open === '*' && leading % 2 === 0) && isWrapped(selected, open, shut)) {
      let inner = selected.slice(open.length, -shut.length)
      if (wrap === '`' && /^ .* $/.test(inner) && inner.trim()) inner = inner.slice(1, -1)
      return { text: src.slice(0, start) + inner + src.slice(end), cursor: start, selection: [start, start + inner.length] }
    }
  }
  let open = wrap, shut = close
  if (wrap === '`') {
    open = shut = '`'.repeat(Math.max(0, ...Array.from(selected.matchAll(/`+/g), match => match[0].length)) + 1)
    if (/^`|`$/.test(selected) || (/^ .* $/.test(selected) && selected.trim())) {
      open += ' '
      shut = ' ' + shut
    }
  } else if (tag && wrap !== '<u>') {
    const space = (ch: string) => !ch || /\s/u.test(ch)
    const punct = (ch: string) => /[\p{P}\p{S}]/u.test(ch)
    const before = Array.from(src.slice(0, start)).at(-1) ?? ''
    const first = Array.from(selected)[0], last = Array.from(selected).at(-1)!
    const after = Array.from(src.slice(end))[0] ?? ''
    const opens = !space(first) && (!punct(first) || space(before) || punct(before))
    const closes = !space(last) && (!punct(last) || space(after) || punct(after))
    // Raw inline HTML preserves exact punctuation without inserting visible spaces.
    if (!opens || !closes || selected.startsWith(wrap) || selected.endsWith(wrap) || (wrap.includes('*') && (before === '*' || after === '*'))) {
      open = `<${tag}>`
      shut = `</${tag}>`
    }
  }
  return {
    text: src.slice(0, start) + open + selected + shut + src.slice(end),
    cursor: start + open.length,
    selection: [start + open.length, end + open.length]
  }
}

/**
 * Toggle a per-line prefix (`> `, `- `, `1. `, `# ` …).
 * If every line already carries it, the prefix is removed instead.
 */
export function toggleLinePrefix(
  src: string,
  start: number,
  end: number,
  prefix: string | ((index: number) => string)
): EditResult {
  const [from, to] = lineBounds(src, start, end)
  const lines = src.slice(from, to).split('\n')
  const resolve = (i: number): string => (typeof prefix === 'function' ? prefix(i) : prefix)
  const has = lines.every((l, i) => l.trimStart().startsWith(resolve(i)))

  const next = lines.map((l, i) => {
    const p = resolve(i)
    if (has) {
      const lead = l.length - l.trimStart().length
      return l.slice(0, lead) + l.trimStart().slice(p.length)
    }
    return p + l
  })

  const joined = next.join('\n')
  return {
    text: src.slice(0, from) + joined + src.slice(to),
    cursor: from,
    selection: [from, from + joined.length]
  }
}

/** Set (or clear with `0`) an ATX heading level on the selected lines. */
export function setHeading(src: string, start: number, end: number, level: number): EditResult {
  const [from, to] = lineBounds(src, start, end)
  const lines = src.slice(from, to).split('\n')
  const next = lines.map((l) => {
    const bare = l.replace(/^\s*#{1,6}\s+/, '')
    return level === 0 ? bare : `${'#'.repeat(level)} ${bare}`
  })
  return { text: src.slice(0, from) + next.join('\n') + src.slice(to), cursor: from }
}

export function insertBlock(src: string, at: number, block: string): EditResult {
  const before = src.slice(0, at)
  const after = src.slice(at)
  // Exactly one blank line on each side — no more, no less.
  const needsBefore = before.length > 0 && !before.endsWith('\n\n')
  const padBefore = needsBefore ? (before.endsWith('\n') ? '\n' : '\n\n') : ''
  const needsAfter = after.length > 0 && !after.startsWith('\n\n')
  const padAfter = needsAfter ? (after.startsWith('\n') ? '\n' : '\n\n') : ''
  const insert = padBefore + block + padAfter
  return { text: before + insert + after, cursor: at + padBefore.length + block.length }
}

/** `**` → `**bold**` etc. The wrapper list defines what the UI can apply. */
export const INLINE_WRAPPERS = {
  bold: '**',
  italic: '*',
  strike: '~~',
  code: '`',
  mark: '==',
  underline: '<u>'
} as const

export interface PrefixToggle {
  id: string
  prefix: string | ((index: number) => string)
  title: string
}

export const LINE_PREFIXES: PrefixToggle[] = [
  { id: 'quote', prefix: '> ', get title() { return t('引用块') } },
  { id: 'ul', prefix: '- ', get title() { return t('无序列表') } },
  { id: 'ol', prefix: (i) => `${i + 1}. `, get title() { return t('有序列表') } },
  { id: 'task', prefix: '- [ ] ', get title() { return t('任务列表') } }
]

/** Smart continuation: what the next line should start with after Enter. */
export function continuationPrefix(line: string): string | null {
  const task = /^(\s*)- \[[ xX]\] /.exec(line)
  if (task) {
    // An empty task item ends the list rather than spawning another.
    return line.trim() === '- [ ]' ? null : `${task[1]}- [ ] `
  }
  const ul = /^(\s*)([-*+]) /.exec(line)
  if (ul) return line.trim() === ul[2] ? null : `${ul[1]}${ul[2]} `
  const ol = /^(\s*)(\d+)([.)]) /.exec(line)
  if (ol) return line.trim() === `${ol[2]}${ol[3]}` ? null : `${ol[1]}${Number(ol[2]) + 1}${ol[3]} `
  const quote = /^(\s*)> ?/.exec(line)
  if (quote) return line.trim() === '>' ? null : `${quote[1]}> `
  return null
}

const PAIRS: Record<string, string> = {
  '*': '*',
  _: '_',
  '`': '`',
  '~': '~',
  '[': ']',
  '(': ')',
  '{': '}',
  '"': '"',
  "'": "'",
  '“': '”',
  '（': '）',
  '【': '】'
}

/**
 * Decide what a typed character should do, before it reaches the document.
 * Returns null to let the character through unchanged.
 */
export function autoPair(
  src: string,
  cursor: number,
  selection: { start: number; end: number } | null,
  ch: string
): EditResult | 'skip-close' | null {
  // Literal Markdown delimiters must remain typeable, including ___ and *** rules.
  if ((!selection || selection.start === selection.end) && '*_~'.includes(ch)) return null
  if (!selection && /['"]/.test(ch) && /[\p{L}\p{N}]/u.test(src.slice(0, cursor).at(-1) ?? '')) return null
  // A fence is typed literally. Pairing its backticks leaves an extra closer
  // inside the new block and moves the caret onto a hidden delimiter.
  const lineStart = src.lastIndexOf('\n', cursor - 1) + 1
  const lineEnd = src.indexOf('\n', cursor)
  if (ch === '`' && !selection && /^ {0,3}`*$/.test(src.slice(lineStart, cursor)) &&
    /^`*$/.test(src.slice(cursor, lineEnd < 0 ? src.length : lineEnd))) return null

  // Closing a pair that is already there should step over it, not double it.
  const closer = Object.values(PAIRS).includes(ch) ? ch : null
  if (closer && src[cursor] === closer && (!selection || selection.start === selection.end)) {
    return 'skip-close'
  }

  const open = PAIRS[ch]
  if (!open) return null

  if (selection && selection.start !== selection.end) {
    const inner = src.slice(selection.start, selection.end)
    return {
      text: src.slice(0, selection.start) + ch + inner + open + src.slice(selection.end),
      cursor: selection.start + ch.length,
      selection: [selection.start + ch.length, selection.end + ch.length]
    }
  }

  return { text: src.slice(0, cursor) + ch + open + src.slice(cursor), cursor: cursor + ch.length }
}

/** Fenced code block helper — adds the language slot and places the cursor. */
export function insertCodeBlock(src: string, at: number, lang = ''): EditResult {
  const block = '```' + lang + '\n\n```'
  const res = insertBlock(src, at, block)
  return { text: res.text, cursor: res.cursor - 3 }
}

export function insertTable(src: string, at: number, rows: number, cols: number): EditResult {
  const head = `| ${Array.from({ length: cols }, (_, i) => t('列{0}', i + 1)).join(' | ')} |`
  const sep = `| ${Array.from({ length: cols }, () => '---').join(' | ')} |`
  const body = Array.from(
    { length: Math.max(rows - 1, 0) },
    () => `| ${Array.from({ length: cols }, () => '   ').join(' | ')} |`
  )
  const block = [head, sep, ...body].join('\n')
  const res = insertBlock(src, at, block)
  return { text: res.text, cursor: res.cursor - block.length + 2 }
}

/** Strip markdown syntax down to readable plain text — for "copy as text". */
export function toPlainText(md: string): string {
  return md
    .replace(/<\/?(?:strong|em|del|mark|u)>/g, '')
    .replace(/^---\n[\s\S]*?\n---\n/, '')
    .replace(/```[\s\S]*?```/g, (m) => m.replace(/```\w*\n?/g, ''))
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/^\s{0,3}#{1,6}\s+/gm, '')
    .replace(/(\*\*|__)(.*?)\1/g, '$2')
    .replace(/(\*|_)(.*?)\1/g, '$2')
    .replace(/~~(.*?)~~/g, '$2')
    .replace(/==(.*?)==/g, '$2')
    .replace(/`([^`]*)`/g, '$1')
    .replace(/^\s*>\s?/gm, '')
    .trim()
}

const WORD = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]|(?:(?![\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}])[\p{L}\p{N}_])+(?:['’.-](?:(?![\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}])[\p{L}\p{N}_])+)*/gu

/** Words as a reader counts them: one per CJK character, one per Latin or number run, never markup. */
export function countWords(md: string): number {
  return toPlainText(md).replace(/<[^>]+>/g, ' ').match(WORD)?.length ?? 0
}

/** `[selected](url)` when a single URL is pasted over a one-line selection. */
export function pasteAsLink(selected: string, pasted: string): string | null {
  const url = pasted.trim()
  if (!selected.trim() || selected.includes('\n') || !/^(?:https?:\/\/|mailto:)\S+$/i.test(url)) return null
  return `[${selected.replace(/[[\]]/g, '\\$&')}](${url.replace(/\(/g, '%28').replace(/\)/g, '%29')})`
}

/** Normalise whitespace and list markers without touching prose content. */
export function tidyMarkdown(src: string): string {
  return src
    .replace(/\r\n/g, '\n')
    .replace(/[ \t]+$/gm, '')
    .replace(/\n{4,}/g, '\n\n\n')
    .replace(/^(\s*)[-*+]\s+/gm, '$1- ')
    .replace(/\n+$/, '\n')
}
