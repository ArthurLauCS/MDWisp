import { t } from '../i18n'
/**
 * The stylesheet for a standalone document.
 *
 * Built from the app's own stylesheets rather than a retyped copy, so an
 * exported file looks like the app and keeps looking like it after the app's
 * styles change.
 *
 * The files are read at runtime instead of imported with `?raw`: a bundler
 * alias can inline them, but Vitest answers that import with an empty string,
 * which would mean the tests ran against a document with no CSS at all. They
 * ship as extra resources (`resources/styles`) and are resolved from the
 * compiled main file, the same way the bundled documents are.
 */
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { createRequire } from 'node:module'
import { app } from 'electron'

const SHEET_NAMES = ['tokens.css', 'fonts.css', 'base.css', 'markdown.css'] as const

/**
 * Walk up until the app root is found, identified by the stylesheets
 * themselves.
 *
 * A fixed relative depth does not survive both homes for this file: at
 * runtime `__dirname` is `out/main`, but under Vitest it is the source
 * directory the test lives in. Searching for the thing being loaded is
 * stable in both and cannot silently point at the wrong tree.
 */
function findStylesDir(startDir: string): string {
  for (let dir = startDir, up = dirname(dir); ; dir = up, up = dirname(dir)) {
    const candidate = join(dir, 'src', 'renderer', 'src', 'styles', SHEET_NAMES[0])
    if (existsSync(candidate)) return dirname(candidate)
    if (dir === up) throw new Error(t('找不到应用的样式表目录'))
  }
}

/**
 * Where the app's stylesheets are at runtime.
 *
 * A packaged build has no source tree, so the stylesheets ship as extra
 * resources; in development they are read from source.
 *
 * `app` is absent when this module is loaded under Vitest, which resolves
 * `electron` to a stub — an unguarded read would throw there and leave the
 * styles untested.
 */
function stylesDir(): string {
  return app?.isPackaged ? join(process.resourcesPath, 'styles') : findStylesDir(__dirname)
}

/** Tests point this at a fixture directory. */
let overrideDir: string | null = null

export function setStylesDirForTest(dir: string | null): void {
  overrideDir = dir
  cache.clear()
}

function trimBase(css: string): string {
  // Everything after this marker belongs to the shell. Removing partial
  // selectors left `*:hover >` attached to the following document rules.
  return css.replace(/\/\* Overlay scrollbars[\s\S]*$/, '')
    .replace(/,\s*#root/g, '')
    .replace(/overflow: hidden;/, '')
}

const cache = new Map<string, string>()

function sheets(): Record<string, string> {
  const dir = overrideDir ?? stylesDir()
  const cached = cache.get(dir)
  if (cached !== undefined) return JSON.parse(cached) as Record<string, string>

  const out: Record<string, string> = {}
  for (const name of SHEET_NAMES) out[name] = readFileSync(join(dir, name), 'utf8')
  out['fonts.css'] = out['fonts.css'].replace(/url\(['"]?\.\/fonts\/([^)'"\s]+)['"]?\)/g, (_match, name: string) => {
    const mime = name.endsWith('.otf') ? 'font/otf' : 'font/ttf'
    return `url('data:${mime};base64,${readFileSync(join(dir, 'fonts', name)).toString('base64')}')`
  })
  cache.set(dir, JSON.stringify(out))
  return out
}

/** KaTeX's sheet with its woff2 fonts inlined; only documents with formulas pay for it. */
export function mathStyles(): string {
  const sheet = createRequire(__filename).resolve('katex/dist/katex.min.css')
  return readFileSync(sheet, 'utf8').replace(/src:url\(fonts\/([\w-]+\.woff2)\)[^;}]*/g, (_match, name: string) =>
    `src:url(data:font/woff2;base64,${readFileSync(join(dirname(sheet), 'fonts', name)).toString('base64')}) format("woff2")`)
}

/**
 * The document stylesheet.
 *
 * `tokens.css` defaults `:root` to dark and overrides it under
 * `[data-theme='light']`, which is exactly the switch a standalone file needs:
 * write the attribute on `<html>` for light, omit it for dark. The palette
 * values are used verbatim.
 */
export function documentStyles(): string {
  const css = sheets()
  return [
    css['tokens.css'],
    css['fonts.css'],
    trimBase(css['base.css']),
    css['markdown.css'],
    `
/* ---- standalone document shell ------------------------------------------ */
html {
  height: auto;
}
body {
  background: var(--bg-base);
  overflow: auto;
  user-select: text;
}
.md {
  padding-block-end: var(--space-16);
}

/* A missing image, drawn rather than broken. Mirrors the app's own
   img.is-missing treatment for a frame the app itself never renders. */
.img-missing {
  margin: var(--space-6) 0;
}
.img-missing__frame,
.md .img-missing__frame.is-missing {
  min-height: 96px;
  border-radius: var(--radius-md);
  border: 1px dashed var(--border-strong);
  background: repeating-linear-gradient(
    -45deg,
    var(--bg-raised),
    var(--bg-raised) 8px,
    var(--bg-inset) 8px,
    var(--bg-inset) 16px
  );
}
.img-missing figcaption {
  margin-top: var(--space-2);
  text-align: center;
  font-family: var(--font-ui);
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}

@media print {
  :root {
    --measure: none;
  }
  .md {
    max-width: none;
    padding: 0;
  }
  .md h1 {
    margin-left: 0;
  }
  /* A long line must wrap instead of being clipped at the page edge. */
  .codeblock__pre {
    overflow-x: visible;
  }
  .codeblock__pre code,
  .md :not(pre) > code {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .codeblock__bar,
  .heading-anchor {
    display: none;
  }
  /* An image straddling a page break is worse than one pushed to the next. */
  .md img,
  .md table,
  .md .codeblock {
    break-inside: avoid;
  }
  .md h1,
  .md h2,
  .md h3 {
    break-after: avoid;
  }
}
`
  ].join('\n')
}
