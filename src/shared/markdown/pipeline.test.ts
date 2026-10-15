import { describe, expect, it } from 'vitest'
import { markdownReferences, renderMarkdown } from './pipeline'

describe('Markdown syntax coverage', () => {
  it.each([
    ['# Heading', '<h1'], ['Heading\n===', '<h1'], ['Heading\n---', '<h2'],
    ['**bold** *italic* ~~strike~~', '<strong>bold</strong>'], ['`code`', '<code>code</code>'],
    ['> quote', '<blockquote>'], ['- item', '<ul>'], ['1. item', '<ol>'],
    ['- [x] done', 'checked'], ['---', '<hr>'], ['line  \nbreak', '<br>'],
    ['```js\nconst x = 1\n```', 'hljs-keyword'], ['    indented', '<code>indented'],
    ['| A | B |\n| --- | --- |\n| 1 | 2 |', '<table>'],
    ['![alt](./image.png)', 'src="./image.png"'], ['[link](README.md)', 'href="README.md"'],
    ['<https://example.com>', 'href="https://example.com"'], ['https://example.com', 'href="https://example.com"'],
    ['==mark==', '<mark>mark</mark>'], ['H~2~O', '<sub>2</sub>'], ['x^2^', '<sup>2</sup>'],
    ['note[^1]\n\n[^1]: Footnote', 'footnote-ref'], ['Term\n: Definition', '<dl>'],
    ['<details><summary>More</summary>Text</details>', '<details>'],
    ['\\*literal\\* &amp;', '*literal* &amp;']
  ])('renders %s', (source, expected) => expect(renderMarkdown(source)).toContain(expected))

  it('resolves reference-style images and links in independently rendered fragments', () => {
    const references = markdownReferences('[doc]: README.md\n[image]: ./image.png "Alt"')
    expect(renderMarkdown('| Doc |\n| --- |\n| [read][doc] |', 'en', references)).toContain('href="README.md"')
    expect(renderMarkdown('![picture][image]', 'en', references)).toContain('src="./image.png"')
    expect(renderMarkdown('```js\nconst x = 1\n```', 'en')).toContain('aria-label="Copy code"')
  })

  it('preserves unsupported extension source instead of claiming rendered output', () => {
    expect(renderMarkdown('[TOC]')).toContain('[TOC]')
    expect(renderMarkdown('```mermaid\ngraph TD\nA --> B\n```')).toContain('<code')
  })
})
