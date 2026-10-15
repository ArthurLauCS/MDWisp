import { describe, expect, it, beforeAll, afterAll } from 'vitest'
import { promises as fs } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { buildHtml } from './html'

let root: string
let docPath: string
let imgDir: string

/** A real 1×1 PNG — a fake byte string would not prove the mime mapping. */
const PNG = Buffer.from(
  '89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de0000000c4944415408d763f8cfc000000301010018dd8db10000000049454e44ae426082',
  'hex'
)

beforeAll(async () => {
  root = await fs.mkdtemp(join(tmpdir(), 'mdview-html-'))
  imgDir = join(root, '发布说明_img')
  await fs.mkdir(imgDir, { recursive: true })
  await fs.writeFile(join(imgDir, '架构草图.png'), PNG)
  docPath = join(root, '发布说明.md')
  await fs.writeFile(docPath, '# 标题\n')
})

afterAll(async () => {
  await fs.rm(root, { recursive: true, force: true })
})

describe('standalone html', () => {
  it('embeds a relative image as a base64 data uri', async () => {
    const r = await buildHtml(docPath, '![架构草图](./发布说明_img/架构草图.png)')
    expect(r.inlined).toHaveLength(1)
    expect(r.html).toContain('data:image/png;base64,')
    expect(r.html).toContain('alt="架构草图"')
    expect(r.html).not.toContain('./发布说明_img/架构草图.png"')
  })

  it('leaves a remote image pointing at its url', async () => {
    const r = await buildHtml(docPath, '![远程](https://example.com/a.png)')
    expect(r.external).toEqual(['https://example.com/a.png'])
    expect(r.html).toContain('src="https://example.com/a.png"')
    expect(r.html).not.toContain('data:image/png;base64,')
  })

  it('leaves a data uri untouched', async () => {
    const uri = 'data:image/gif;base64,R0lGOD'
    const r = await buildHtml(docPath, `![内联](${uri})`)
    expect(r.html).toContain(uri)
    expect(r.inlined).toEqual([])
  })

  it('replaces a missing image with a styled placeholder carrying the alt', async () => {
    const r = await buildHtml(docPath, '![找不到的图](./发布说明_img/不存在.png)')
    expect(r.html).toContain('img-missing__frame')
    expect(r.html).toContain('缺失：找不到的图')
    expect(r.missing).toHaveLength(1)
  })

  it('falls back to the filename when a missing image has no alt', async () => {
    const r = await buildHtml(docPath, '![](./发布说明_img/不存在.png)')
    expect(r.html).toContain('缺失：不存在.png')
  })

  it('treats a windows absolute path as local, not as a url scheme', async () => {
    const r = await buildHtml(docPath, '![截图](C:\\Users\\me\\Pictures\\shot.png)')
    expect(r.external).toEqual([])
    expect(r.missing).toHaveLength(1)
    expect(r.html).not.toContain('C:\\Users')
  })

  it('resolves an absolute path that does exist', async () => {
    const abs = join(imgDir, '架构草图.png')
    const r = await buildHtml(docPath, `![草图](${abs})`)
    expect(r.inlined).toHaveLength(1)
  })

  it('tolerates a percent-encoded chinese relative path', async () => {
    const r = await buildHtml(
      docPath,
      '![草图](./%E5%8F%91%E5%B8%83%E8%AF%B4%E6%98%8E_img/%E6%9E%B6%E6%9E%84%E8%8D%89%E5%9B%BE.png)'
    )
    expect(r.inlined).toHaveLength(1)
  })

  it('produces a document that stands alone', async () => {
    const r = await buildHtml(docPath, '# 标题\n\n正文')
    expect(r.html.startsWith('<!doctype html>')).toBe(true)
    expect(r.html).toContain('<style>')
    expect(r.html).toContain('--accent:')
    // No script, no external stylesheet, no font request.
    expect(r.html).not.toContain('<script')
    expect(r.html).not.toContain('<link')
    expect(r.html).not.toMatch(/https?:\/\/[^"' ]*\.(css|js)/)
    expect(r.html).toContain('data:font/otf;base64,')
    expect(r.html).toContain('data:font/ttf;base64,')
    expect(r.html).not.toContain("url('./fonts/")
    expect(r.html).not.toContain('*:hover >')
    expect(r.html).not.toMatch(/body,\s*\{/)
  })

  it('embeds formula fonts only for documents with math', async () => {
    const math = await buildHtml(docPath, '$$\nE = mc^2\n$$')
    expect(math.html).toContain('katex-display')
    expect(math.html).toContain('data:font/woff2;base64,')
    expect(math.html).not.toMatch(/url\(fonts\//)
    expect((await buildHtml(docPath, '# 标题')).html).not.toContain('KaTeX_Main')
  })

  it('keeps the app code-block markup so the two renderings agree', async () => {
    const r = await buildHtml(docPath, '```\ninterface X { a: string }\n```')
    expect(r.html).toContain('codeblock__lang')
    expect(r.html).toContain('data-lang="typescript"')
  })

  it('switches theme by attribute, with dark values left as the default', async () => {
    const dark = await buildHtml(docPath, '# 标题')
    const light = await buildHtml(docPath, '# 标题', 'light')
    expect(light.html).toContain('<html lang="zh-CN" data-theme="light">')
    expect(dark.html).toContain('<html lang="zh-CN">')
    expect(dark.html).toContain("[data-theme='light']")
  })
})
