/**
 * The one markdown pipeline, shared by the reading view and by export.
 *
 * It lives in `shared/` rather than under the renderer because the main
 * process renders the same documents for HTML and PDF export. Two copies of
 * this config would drift, and a document that looks one way in the app and
 * another way in an exported file is a bug nobody notices until it ships.
 */
import MarkdownIt from 'markdown-it'
import anchor from 'markdown-it-anchor'
import footnote from 'markdown-it-footnote'
import taskLists from 'markdown-it-task-lists'
import deflist from 'markdown-it-deflist'
import mark from 'markdown-it-mark'
import sub from 'markdown-it-sub'
import sup from 'markdown-it-sup'
import hljs from 'highlight.js/lib/common'
import { detectLanguage } from './detect'
import { mathPlugin } from './math'
import { frontmatterEnd } from './frontmatter'
import type { Locale } from '../types'
import { en } from '../locales/en'
import { zhCN } from '../locales/zh-CN'

const md = new MarkdownIt({
  html: true,
  linkify: true,
  typographer: true,
  breaks: false,
  highlight(code, langHint): string {
    // Explicit hint wins; otherwise the local detector picks, and the scored
    // result is written onto the block so the UI can show and correct it.
    let lang = langHint.trim()
    let confidence: number | null = null

    if (!lang) {
      const guess = detectLanguage(code)
      lang = guess.lang ?? ''
      confidence = guess.confidence
    }

    let body: string
    if (lang && hljs.getLanguage(lang)) {
      try {
        body = hljs.highlight(code, { language: lang, ignoreIllegals: true }).value
      } catch {
        body = md.utils.escapeHtml(code)
      }
    } else {
      body = md.utils.escapeHtml(code)
    }

    const label = lang || 'text'
    const conf = confidence !== null ? ` data-confidence="${Math.round(confidence)}"` : ''
    return (
      `<div class="codeblock" data-lang="${md.utils.escapeHtml(label)}"${conf}>` +
      `<div class="codeblock__bar">` +
      `<button class="codeblock__lang" type="button" data-action="pick-language">${md.utils.escapeHtml(label)}` +
      (confidence !== null ? `<span class="codeblock__conf">${Math.round(confidence)}%</span>` : '') +
      `</button>` +
      `<button class="codeblock__copy" type="button" data-action="copy-code" aria-label="复制代码"></button>` +
      `</div>` +
      `<pre class="codeblock__pre"><code class="hljs language-${md.utils.escapeHtml(label)}">${body}</code></pre>` +
      `</div>`
    )
  }
})

export { md as markdownParser }

export type MarkdownReferences = Record<string, { href: string; title: string }>

export function markdownReferences(source: string): MarkdownReferences {
  if (!source.includes(']:')) return {}
  const env: { references?: MarkdownReferences } = {}
  md.parse(source.slice(frontmatterEnd(source)), env)
  return env.references ?? {}
}

md.use(anchor, {
  permalink: anchor.permalink.linkInsideHeader({
    symbol: '#',
    placement: 'before',
    class: 'heading-anchor',
    ariaHidden: true
  }),
  slugify: (s) =>
    encodeURIComponent(
      s
        .trim()
        .toLowerCase()
        .replace(/[\s]+/g, '-')
        .replace(/[^\p{L}\p{N}\-_]/gu, '')
    )
})
md.use(footnote).use(taskLists, { label: true }).use(deflist).use(mark).use(sub).use(sup).use(mathPlugin)

const fence = md.renderer.rules.fence!
md.renderer.rules.fence = (tokens, index, options, env, self) => fence(tokens, index, options, env, self)
  .replace('aria-label="复制代码"', `aria-label="${(env.language === 'en' ? en : zhCN)['复制代码']}"`)

/** Rendering is pure and cheap enough to run per keystroke on normal docs. */
export function renderMarkdown(source: string, language: Locale = 'zh-CN', references: MarkdownReferences = {}): string {
  return md.render(source.slice(frontmatterEnd(source)), { language, references: { ...references } })
}
