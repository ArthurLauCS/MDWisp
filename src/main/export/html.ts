import { currentLanguage, t } from '../i18n'
/**
 * Build one self-contained HTML file from a document.
 *
 * Self-contained means what it says: after this runs, opening the file makes
 * no network requests and needs no sibling folder. Every local image is
 * base64 in the markup, the stylesheet is inline, and a script would be an
 * admission of failure — there is none.
 */
import { promises as fs } from 'node:fs'
import { basename, extname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { renderMarkdown } from '@shared/markdown/pipeline'
import { documentStyles, mathStyles } from './css'

const MIME: Record<string, string> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
  '.bmp': 'image/bmp',
  '.svg': 'image/svg+xml'
}

/** Escape for text and attribute contexts alike; the two overlap enough. */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function escapeAttr(text: string): string {
  return escapeHtml(text).replace(/'/g, '&#39;')
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}

export interface HtmlBuild {
  html: string
  /** Local images that were embedded, by document-relative path. */
  inlined: string[]
  /** Local images that could not be read, with a reason for the UI. */
  missing: { path: string; reason: string }[]
  /** Images left pointing at a remote URL. */
  external: string[]
  bytes: number
}

/**
 * A missing asset is a designed state, not a broken-image glyph. The frame
 * carries the app's own `is-missing` class so the two renderings agree.
 */
function placeholder(alt: string, src: string): string {
  const label = alt || basename(safeDecode(src).split('\\').join('/')) || t('图片')
  return (
    `<figure class="img-missing">` +
    `<div class="img-missing__frame is-missing" role="img" aria-label="${escapeAttr(label)}"></div>` +
    t('<figcaption>缺失：{0}</figcaption>', escapeHtml(label)) +
    `</figure>`
  )
}

const IMG_TAG = /<img\b([^>]*?)\bsrc=(["'])([^"']+)\2([^>]*)>/gi

/**
 * What the standalone file can do with an image source.
 *
 * `remote` — a real URL the recipient's browser will fetch, left alone.
 * `local`  — a candidate on this disk. It may or may not exist; a relative
 *            path resolves against the document folder, an absolute one is
 *            used as written (rewriting it would point at nothing). Either
 *            way it is *attempted*, because failing to try leaves a dead
 *            reference in the file, which is the failure this mode exists to
 *            prevent. A `null` from here means "not ours" — `data:` and real
 *            URLs.
 */
function classify(src: string): { kind: 'remote' } | { kind: 'local'; abs: string } {
  if (/^data:/i.test(src)) return { kind: 'remote' }
  if (/^file:\/\//i.test(src)) {
    try {
      return { kind: 'local', abs: fileURLToPath(src) }
    } catch {
      return { kind: 'remote' }
    }
  }
  // A drive letter looks exactly like a URL scheme (`C:\...`), so single-letter
  // schemes are never treated as remote.
  if (/^[a-z]:[\\/]/i.test(src) || src.startsWith('\\\\')) return { kind: 'local', abs: src }
  if (/^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith('//')) return { kind: 'remote' }
  return { kind: 'local', abs: '' }
}

/** Strip a query or fragment, which belong to the URL rather than the file. */
function splitSuffix(src: string): [string, string] {
  const at = src.search(/[?#]/)
  if (at === -1) return [src, '']
  return [src.slice(0, at), src.slice(at)]
}

async function embedImages(
  html: string,
  docDir: string
): Promise<{ html: string; build: Omit<HtmlBuild, 'html' | 'bytes'> }> {
  const inlined: string[] = []
  const missing: { path: string; reason: string }[] = []
  const external: string[] = []

  // Collect matches first: the replacer must stay synchronous, and reading
  // every image up front lets them all be read in parallel.
  const found = [...html.matchAll(IMG_TAG)]
  const contents = await Promise.all(
    found.map(async (m) => {
      const src = m[3]
      const [pathPart, suffix] = splitSuffix(src)
      const where = classify(safeDecode(pathPart))
      if (where.kind === 'remote') {
        external.push(src)
        return { m, verdict: 'remote' as const }
      }

      const abs = where.abs || join(docDir, safeDecode(pathPart))
      try {
        return { m, verdict: 'embedded' as const, src, abs, data: await fs.readFile(abs) }
      } catch {
        return { m, verdict: 'missing' as const, src, abs, suffix }
      }
    })
  )

  const replacements = new Map<string, string>()
  for (const item of contents) {
    const whole = item.m[0]
    const alt = /\balt=(["'])(.*?)\1/i.exec(whole)?.[2] ?? ''

    if (item.verdict === 'remote') continue

    if (item.verdict === 'missing') {
      missing.push({ path: item.abs, reason: t('本地图片读不到，已用占位符代替') })
      replacements.set(whole, placeholder(alt, item.src))
      continue
    }

    const mime = MIME[extname(item.abs).toLowerCase()] ?? 'application/octet-stream'
    inlined.push(item.src)
    replacements.set(
      whole,
      `<img src="data:${mime};base64,${item.data.toString('base64')}"` +
        ` alt="${escapeAttr(alt)}" data-source="${escapeAttr(item.src)}">`
    )
  }

  let out = html
  for (const [from, to] of replacements) out = out.split(from).join(to)
  return { html: out, build: { inlined, missing, external } }
}

export async function buildHtml(
  docPath: string,
  source: string,
  theme: 'dark' | 'light' = 'dark'
): Promise<HtmlBuild> {
  const docDir = join(docPath, '..')
  const language = currentLanguage()
  const { html: body, build } = await embedImages(renderMarkdown(source, language), docDir)

  const title = basename(docPath).replace(/\.(md|markdown|mdx)$/i, '')
  const themeAttr = theme === 'light' ? ` data-theme="light"` : ''

  const html = `<!doctype html>
<html lang="${language}"${themeAttr}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="MDWisp">
<title>${escapeHtml(title)}</title>
<style>
${documentStyles()}
${body.includes('class="katex') ? mathStyles() : ''}
</style>
</head>
<body>
<article class="md">
${body}
</article>
</body>
</html>
`

  return { html, ...build, bytes: Buffer.byteLength(html, 'utf8') }
}
