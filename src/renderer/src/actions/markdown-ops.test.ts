import { describe, expect, it } from 'vitest'
import { autoPair, countWords, minimalChange, pasteAsLink, toggleInline, toggleLinePrefix, setHeading, toPlainText } from './markdown-ops'
import { renderMarkdown } from '@shared/markdown/pipeline'

const formats = [['**', 'strong'], ['*', 'em'], ['~~', 's'], ['==', 'mark'], ['<u>', 'u']] as const
const examples = [
  ['复审：保留', 0, 3], ['复审： 保留', 0, 4], ['复审:保留', 0, 3],
  ['Review:keep', 0, 7], ['前（保留）后', 1, 5], ['前，后', 1, 2],
  ['前：保留', 1, 4], ['hello world!', 6, 12], ['  word  ', 0, 8],
  ['中文English123混排', 2, 12], ['emoji🙂next', 5, 7], ['日本語：保持', 0, 4],
  ['한국어：유지', 0, 4], ['café déjà vu', 0, 4], ['a\tword\tb', 1, 7],
  ['(word)', 1, 5], ['word.', 0, 4], ['hello *world*', 0, 13]
] as const

describe('formatting real prose without changing its text', () => {
  for (const [wrap, tag] of formats) for (const [source, start, end] of examples) {
    it(`${wrap}: ${JSON.stringify(source)} [${start}, ${end}]`, () => {
      const result = toggleInline(source, start, end, wrap)
      const html = renderMarkdown(result.text)
      // <del> is the standards-compatible fallback for Markdown strikethrough.
      expect(html).toMatch(new RegExp(`<${tag === 's' ? '(s|del)' : tag}>`))
      const [from, to] = result.selection!
      expect(result.text.slice(from, to)).toBe(source.slice(start, end).trim())
      const undone = toggleInline(result.text, from, to, wrap)
      expect(undone.text).toBe(source)
    })
  }
  for (const wrap of ['**', '*', '~~', '==', '<u>', '`']) {
    it(`formats and toggles multiple paragraphs with ${wrap}`, () => {
      const source = 'first\n\n中文： 保留\nlast'
      const result = toggleInline(source, 0, source.length, wrap)
      expect(result.text.split('\n')[1]).toBe('')
      expect(toggleInline(result.text, ...result.selection!, wrap).text).toBe(source)
    })
    it(`supports an empty selection with ${wrap}`, () => {
      const result = toggleInline('ab', 1, 1, wrap)
      expect(result.text[result.cursor - 1]).toBe(wrap.at(-1))
      expect(result.text.startsWith('a')).toBe(true)
      expect(result.text.endsWith('b')).toBe(true)
    })
  }
  it('does not turn a bold selection into malformed italic markers', () => {
    expect(renderMarkdown(toggleInline('**word**', 0, 8, '*').text)).toContain('<em><strong>word</strong></em>')
    const result = toggleInline('**word**', 2, 6, '*')
    expect(renderMarkdown(result.text)).toContain('<em>word</em>')
    expect(toggleInline(result.text, ...result.selection!, '*').text).toBe('**word**')
    expect(toggleInline('***word***', 3, 7, '*').text).toBe('**word**')
    expect(toggleInline('***word***', 3, 7, '**').text).toBe('*word*')
  })
  it.each(['', ' ', '\t  '])('leaves blank selections unchanged: %j', source => {
    if (source) expect(toggleInline(source, 0, source.length, '**').text).toBe(source)
  })
  it('removes existing code formatting and ignores whitespace-only code selections', () => {
    expect(toggleInline('`word`', 0, 6, '`').text).toBe('word')
    expect(toggleInline('   ', 0, 3, '`').text).toBe('   ')
  })
  it('does not include the following line in line-format shortcuts', () => {
    expect(toggleLinePrefix('first\nsecond', 0, 6, '> ').text).toBe('> first\nsecond')
    expect(setHeading('first\nsecond', 0, 6, 2).text).toBe('## first\nsecond')
    expect(setHeading('\nsecond', 0, 0, 2).text).toBe('## \nsecond')
  })
  for (const context of ['# 复审：保留', '- 复审：保留', '> 复审：保留', '- [ ] 复审：保留', '[复审：保留](./doc.md)', '| 内容 |\n| --- |\n| 复审：保留 |']) {
    it(`formats punctuation inside a Markdown container: ${context}`, () => {
      const start = context.indexOf('复审：')
      const result = toggleInline(context, start, start + 3, '**')
      expect(renderMarkdown(result.text)).toContain('<strong>复审：</strong>保留')
      expect(toggleInline(result.text, ...result.selection!, '**').text).toBe(context)
    })
  }
  for (const [source, wrap] of [['**one** **two**', '**'], ['*one* *two*', '*'], ['<u>one</u> <u>two</u>', '<u>']]) {
    it(`does not mistake separate spans for one wrapper: ${source}`, () => {
      const result = toggleInline(source, 0, source.length, wrap)
      expect(renderMarkdown(result.text)).not.toMatch(/\*|&lt;|&gt;/)
      expect(toggleInline(result.text, ...result.selection!, wrap).text).toBe(source)
    })
  }
  it('copies HTML fallback emphasis as plain text', () => {
    expect(toPlainText(toggleInline('复审：保留', 0, 3, '**').text)).toBe('复审：保留')
  })
  it.each(['a`b', '``', 'a``b`c', ' spaced ', 'const x = "a_b"', '中文`代码`'])('round trips inline code: %j', source => {
    const result = toggleInline(source, 0, source.length, '`')
    expect(renderMarkdown(result.text)).toContain('<code>')
    expect(renderMarkdown(result.text)).toContain(source.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'))
    expect(toggleInline(result.text, ...result.selection!, '`').text).toBe(source)
  })
})

describe('typing and local edits', () => {
  it.each(['_', '*', '~'])('types %s literally without a selection', ch => {
    for (const source of ['', 'snake', '__', '**', '---\n', '前缀']) expect(autoPair(source, source.length, null, ch)).toBeNull()
  })
  it('keeps selected-text pairing and bracket pairing', () => {
    expect(autoPair('word', 0, { start: 0, end: 4 }, '_')).toMatchObject({ text: '_word_' })
    expect(autoPair('tail', 0, null, '(')).toMatchObject({ text: '()tail', cursor: 1 })
    expect(autoPair('()', 1, null, ')')).toBe('skip-close')
    expect(autoPair('don', 3, null, "'")).toBeNull()
  })
  it.each([
    ['a\nb\nc', 'a\nb()\nc', { from: 3, to: 3, insert: '()' }],
    ['abc', 'ac', { from: 1, to: 2, insert: '' }],
    ['', '_', { from: 0, to: 0, insert: '_' }],
    ['same', 'same', { from: 4, to: 4, insert: '' }]
  ])('keeps untouched document ranges: %s', (source, next, expected) => {
    expect(minimalChange(source as string, next as string)).toEqual(expected)
  })
})

describe('countWords', () => {
  it.each([
    ['', 0],
    ['写作，从这里开始。', 7],
    ['Hello world, it’s 2026.', 4],
    ['# 标题 Title\n\n**加粗** and [链接](https://example.com/a-b)', 8],
    ['| 名称 | 数量 |\n| --- | --- |\n| 苹果<br>梨 | 2 |', 8],
    ['混排English单词', 5]
  ])('%s → %i', (source, expected) => {
    expect(countWords(source)).toBe(expected)
  })
})

describe('pasteAsLink', () => {
  it('links a selection to a pasted URL only', () => {
    expect(pasteAsLink('MDWisp', ' https://github.com/x \n')).toBe('[MDWisp](https://github.com/x)')
    expect(pasteAsLink('a [b]', 'https://e.com/(1)')).toBe('[a \\[b\\]](https://e.com/%281%29)')
    expect(pasteAsLink('', 'https://e.com')).toBeNull()
    expect(pasteAsLink('two\nlines', 'https://e.com')).toBeNull()
    expect(pasteAsLink('text', 'not a url')).toBeNull()
    expect(pasteAsLink('text', 'https://a.com https://b.com')).toBeNull()
  })
})
