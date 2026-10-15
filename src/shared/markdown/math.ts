/**
 * `$inline$` and `$$display$$` math, shared by export and the live editor so
 * both agree on what counts as a formula.
 */
import katex from 'katex'
import type MarkdownIt from 'markdown-it'

const rendered = new Map<string, string>()

/** Cached: the live editor re-renders every formula on each keystroke. */
export function renderMath(tex: string, displayMode: boolean): string {
  const key = (displayMode ? '$$' : '$') + tex
  let html = rendered.get(key)
  if (html === undefined) {
    // ponytail: wholesale clear instead of LRU; formulas being typed are re-rendered once.
    if (rendered.size > 1000) rendered.clear()
    html = katex.renderToString(tex, { displayMode, throwOnError: false })
    rendered.set(key, html)
  }
  return html
}

/**
 * Pandoc's rule: the opener hugs its content and the closer is not followed
 * by a digit, so prices such as "$5 and $10" stay plain text.
 */
export function inlineMathEnd(src: string, start: number): number {
  if (src[start] !== '$' || src[start + 1] === '$' || !/\S/.test(src[start + 1] ?? ' ')) return -1
  for (let i = start + 1; i < src.length; i++) {
    if (src[i] === '\\') i++
    else if (src[i] === '$' && /\S/.test(src[i - 1]) && !/\d/.test(src[i + 1] ?? '')) return i
  }
  return -1
}

/** Every inline formula in `text`, as `[from, to)` including the dollars. */
export function findInlineMath(text: string, skip: (at: number) => boolean = () => false): { from: number; to: number; tex: string }[] {
  const found: { from: number; to: number; tex: string }[] = []
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '\\') { i++; continue }
    if (text[i] !== '$') continue
    const end = inlineMathEnd(text, i)
    if (end < 0) {
      if (text[i + 1] === '$') i++
      continue
    }
    if (!skip(i)) found.push({ from: i, to: end + 1, tex: text.slice(i + 1, end) })
    i = end
  }
  return found
}

/** The formula of a paragraph that is a `$$ … $$` block, or null. */
export function blockMath(text: string): string | null {
  const match = /^ {0,3}\$\$([\s\S]*?)\$\$[ \t]*$/.exec(text)
  return match && match[1].trim() ? match[1].trim() : null
}

export function mathPlugin(md: MarkdownIt): void {
  md.inline.ruler.after('escape', 'math_inline', (state, silent) => {
    // Consume `$$` whole, as findInlineMath does, so it never opens at its second dollar.
    if (state.src.startsWith('$$', state.pos)) {
      if (!silent) state.pending += '$$'
      state.pos += 2
      return true
    }
    const end = inlineMathEnd(state.src, state.pos)
    if (end < 0) return false
    if (!silent) state.push('math_inline', 'math', 0).content = state.src.slice(state.pos + 1, end)
    state.pos = end + 1
    return true
  })
  md.block.ruler.before('fence', 'math_block', (state, startLine, endLine, silent) => {
    if (state.sCount[startLine] - state.blkIndent >= 4) return false
    const start = state.bMarks[startLine] + state.tShift[startLine]
    if (state.src.slice(start, start + 2) !== '$$') return false
    const lines = [state.src.slice(start + 2, state.eMarks[startLine])]
    let line = startLine
    // A blank line ends the attempt, matching the editor's paragraph-based view.
    while (!lines[lines.length - 1].trimEnd().endsWith('$$')) {
      if (++line >= endLine || state.isEmpty(line)) return false
      lines.push(state.src.slice(state.bMarks[line] + state.tShift[line], state.eMarks[line]))
    }
    if (silent) return true
    const last = lines.length - 1
    lines[last] = lines[last].trimEnd().slice(0, -2)
    state.line = line + 1
    const token = state.push('math_block', 'math', 0)
    token.block = true
    token.content = lines.join('\n').trim()
    token.map = [startLine, state.line]
    return true
  })
  md.renderer.rules.math_inline = (tokens, index) => renderMath(tokens[index].content, false)
  md.renderer.rules.math_block = (tokens, index) => `<div class="math-block">${renderMath(tokens[index].content, true)}</div>\n`
}
