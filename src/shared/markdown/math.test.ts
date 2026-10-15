import { describe, expect, it } from 'vitest'
import { blockMath, findInlineMath } from './math'
import { renderMarkdown } from './pipeline'

describe('math', () => {
  it('finds inline formulas but leaves prices and escapes alone', () => {
    expect(findInlineMath('a $x^2$ b $y$').map(m => m.tex)).toEqual(['x^2', 'y'])
    expect(findInlineMath('costs $5 and $10 today')).toEqual([])
    expect(findInlineMath('\\$x$ and $ x$')).toEqual([])
    expect(findInlineMath('$a\\$b$')).toEqual([{ from: 0, to: 6, tex: 'a\\$b' }])
    expect(findInlineMath('$x$ $y$', at => at === 0).map(m => m.tex)).toEqual(['y'])
    expect(findInlineMath('say $$x$$ inline')).toEqual([])
  })

  it('recognises a paragraph that is a display block', () => {
    expect(blockMath('$$\nE = mc^2\n$$')).toBe('E = mc^2')
    expect(blockMath('$$a+b$$')).toBe('a+b')
    expect(blockMath('$$ $$')).toBeNull()
    expect(blockMath('$$a$$ and more')).toBeNull()
  })

  it('renders through the shared pipeline, outside code only', () => {
    expect(renderMarkdown('energy $E=mc^2$')).toContain('class="katex"')
    expect(renderMarkdown('$$\n\\frac{a}{b}\n$$')).toContain('class="katex-display"')
    expect(renderMarkdown('$$x$$')).toContain('class="math-block"')
    expect(renderMarkdown('`$x$`')).not.toContain('katex')
    expect(renderMarkdown('```\n$$\nx\n$$\n```')).not.toContain('katex')
    expect(renderMarkdown('$5 and $10')).not.toContain('katex')
    expect(renderMarkdown('say $$x$$ inline')).not.toContain('katex')
    expect(renderMarkdown('$$\nopen\n\nnever closed $$')).not.toContain('katex-display')
    expect(renderMarkdown('| a |\n| --- |\n| $x$ |')).toContain('katex')
  })

  it('shows a malformed formula instead of throwing', () => {
    expect(renderMarkdown('$\\frac{$')).toContain('katex-error')
  })
})
