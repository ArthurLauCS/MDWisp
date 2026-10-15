import { t } from '../i18n'
import { patchSettings, settingsSnapshot } from '../state/settings'
import { togglePanel } from '../state/ui'
import { currentDocument, newDocument, openDocumentDialog, saveDocument } from '../state/documents'
import { openWorkspaceDialog } from '../state/workspace'
import { linkAt, unlinkAt } from '../markdown/links'
import { followDocumentLink } from '../markdown/navigation'
import type { ActionContext, ActionDef } from './types'
import { DEFAULT_SETTINGS } from '@shared/types'
import {
  autoPair,
  INLINE_WRAPPERS,
  insertBlock,
  insertCodeBlock,
  insertTable,
  LINE_PREFIXES,
  setHeading,
  tidyMarkdown,
  toPlainText,
  toggleInline,
  toggleLinePrefix
} from './markdown-ops'

/**
 * Every user-visible operation in the app.
 *
 * `group` decides which context-menu section an item lands in; the order of
 * `GROUP_ORDER` decides the section order. Nothing here knows about React,
 * which is what lets the palette, the menus and the key handler share it.
 */
export const GROUPS = [
  'format',
  'convert',
  'insert',
  'find',
  'clipboard',
  'table-row',
  'table-col',
  'table-cell',
  'table-data',
  'table-style',
  'table-struct',
  'code',
  'image',
  'link',
  'file',
  'view'
] as const

export type GroupId = (typeof GROUPS)[number]

export const GROUP_LABELS: Record<GroupId, string> = {
  get format() { return t('格式') },
  get convert() { return t('转换为') },
  get insert() { return t('插入') },
  get find() { return t('查找') },
  get clipboard() { return t('剪贴板') },
  get 'table-row'() { return t('行') },
  get 'table-col'() { return t('列') },
  get 'table-cell'() { return t('单元格') },
  get 'table-data'() { return t('数据') },
  get 'table-style'() { return t('样式') },
  get 'table-struct'() { return t('结构') },
  get code() { return t('代码') },
  get image() { return t('图片') },
  get link() { return t('链接') },
  get file() { return t('文件') },
  get view() { return t('视图') }
}

/** Wrappers exposed as one action each so a key can bind to any of them. */
const inline = (
  id: string,
  title: string,
  key: string,
  wrap: string,
  keywords: string[]
): ActionDef => ({
  id,
  get title() { return t(title) },
  keywords,
  scope: 'document',
  key,
  group: 'format',
  run: (ctx) => {
    const { start, end } = ctx.selection ?? { start: ctx.cursor, end: ctx.cursor }
    const res = toggleInline(ctx.source, start, end, wrap)
    ctx.replace(0, ctx.source.length, res.text, res.cursor)
    if (res.selection) ctx.select(res.selection[0], res.selection[1])
  }
})

const prefix = (toggle: (typeof LINE_PREFIXES)[number], key?: string): ActionDef => ({
  id: `prefix.${toggle.id}`,
  get title() { return toggle.title },
  keywords: ['list', 'quote', 'bullet', 'ordered', 'task'],
  scope: 'document',
  key,
  group: 'convert',
  run: (ctx) => {
    const { start, end } = ctx.selection ?? { start: ctx.cursor, end: ctx.cursor }
    const res = toggleLinePrefix(ctx.source, start, end, toggle.prefix)
    ctx.replace(0, ctx.source.length, res.text, res.cursor)
    if (res.selection) ctx.select(res.selection[0], res.selection[1])
  }
})

const heading = (level: number): ActionDef => ({
  id: `heading.${level}`,
  get title() { return level === 0 ? t('正文段落') : t('标题 H{0}', level) },
  keywords: ['heading', 'title', `h${level}`],
  scope: 'document',
  key: level > 0 ? `Ctrl+Shift+Digit${level}` : 'Ctrl+Shift+Digit0',
  group: 'convert',
  run: (ctx) => {
    const { start, end } = ctx.selection ?? { start: ctx.cursor, end: ctx.cursor }
    const res = setHeading(ctx.source, start, end, level)
    ctx.replace(0, ctx.source.length, res.text, res.cursor)
  }
})

/** Alt text from the original filename — never from the hashed stored name. */
function altTextFor(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? ''
  return base.replace(/\.[^.]+$/, '') || t('图片')
}

/**
 * A table action whose behaviour lives in the engine's spec table.
 *
 * The id is the whole contract: `tableui/specs.ts` maps it to a matrix
 * transformation, and `runTableAction` applies it. The action stays disabled
 * until the caret is actually inside a table, so the menu explains itself
 * instead of offering something that would silently do nothing.
 */
const tableAction = (
  id: string,
  title: string,
  key: string,
  group: GroupId,
  keywords: string[] = []
): ActionDef => ({
  id,
  get title() { return t(title) },
  keywords,
  scope: 'table',
  key,
  group,
  enabled: (ctx) => ctx.inTable,
  disabledReason: (ctx) => (ctx.inTable ? undefined : t('光标不在表格内')),
  run: async (ctx) => {
    const { runTableAction, needsClipboard } = await import('../tableui/run-spec')
    const text = needsClipboard(id) ? await window.mdview.clipboard.readTable() : ''
    if (!runTableAction(id, ctx, text)) ctx.toast(t('这个操作在当前单元格不可用'))
  }
})

/**
 * A table action the menus should still advertise but which has no engine
 * implementation yet. Each states its own reason, so a greyed-out row is an
 * explanation rather than a dead end.
 */
const tableStub = (
  id: string,
  title: string,
  key: string,
  group: GroupId,
  keywords: string[] = []
): ActionDef => ({
  id,
  get title() { return t(title) },
  keywords,
  scope: 'table',
  key,
  group,
  enabled: () => false,
  disabledReason: () => t('这个操作还没实现'),
  run: () => undefined
})

const zoom = (id: string, title: string, key: string, size: (current: number) => number): ActionDef => ({
  id,
  get title() { return t(title) },
  keywords: ['zoom', 'font size', '字号', '缩放'],
  scope: 'app',
  key,
  group: 'view',
  run: (ctx) => {
    // Same bounds as the font size field in settings.
    const fontSize = Math.min(26, Math.max(12, size(settingsSnapshot().fontSize)))
    void patchSettings({ fontSize })
    ctx.toast(t('正文字号 {0}px', fontSize))
  }
})

export const ACTIONS: ActionDef[] = [
  { id: 'document.new', get title() { return t('新建文档') }, key: 'Ctrl+N', scope: 'app', group: 'file', keywords: ['new', 'blank', '草稿'], run: () => newDocument() },
  { id: 'document.open', get title() { return t('打开文档') }, key: 'Ctrl+O', scope: 'app', group: 'file', keywords: ['open'], run: () => openDocumentDialog() },
  { id: 'workspace.open', get title() { return t('打开目录') }, key: 'Ctrl+Shift+O', scope: 'app', group: 'file', run: () => openWorkspaceDialog() },
  { id: 'view.settings', get title() { return t('设置') }, key: 'Ctrl+,', scope: 'app', group: 'view', run: () => togglePanel('settings') },
  { id: 'view.shortcuts', get title() { return t('快捷键') }, key: 'F1', scope: 'app', group: 'view', run: () => togglePanel('shortcuts') },
  { id: 'view.help', get title() { return t('使用说明') }, scope: 'app', group: 'view', run: () => togglePanel('help') },
  { id: 'view.palette', get title() { return t('命令面板') }, key: 'Ctrl+P', scope: 'app', group: 'view', run: () => togglePanel('palette') },
  { id: 'document.export', get title() { return t('导出文档') }, key: 'Ctrl+Shift+E', scope: 'app', group: 'file', run: () => togglePanel('export') },
  {
    id: 'view.history', get title() { return t('历史版本') }, key: 'Ctrl+Alt+Y',
    scope: 'app', group: 'view', keywords: ['history', '快照', '还原'],
    run: () => togglePanel('history')
  },
  {
    id: 'document.save', get title() { return t('保存文档') }, key: 'Ctrl+S', scope: 'document', group: 'file',
    run: async () => { await saveDocument() }
  },
  {
    id: 'document.reload', get title() { return t('重新载入磁盘版本') }, key: 'Ctrl+Shift+R', scope: 'document', group: 'file',
    keywords: ['reload', 'revert', 'disk', '还原', '外部修改'],
    enabled: () => !!currentDocument()?.meta.path,
    disabledReason: () => t('请先保存文档'),
    run: (ctx) => ctx.reloadFromDisk?.()
  },
  {
    id: 'document.organize', get title() { return t('整理为文档文件夹') }, scope: 'document', group: 'file',
    keywords: ['organize', 'folder', '文件夹', '散装', '转换'],
    enabled: () => !!currentDocument()?.meta.path && !currentDocument()!.meta.inFolder,
    disabledReason: () => (currentDocument()?.meta.path ? t('这份文档已经在自己的文档文件夹里') : t('请先保存文档')),
    run: () => togglePanel('organize')
  },
  {
    id: 'document.undo', get title() { return t('撤销') }, key: 'Ctrl+Z', scope: 'document', group: 'file',
    run: (ctx) => ctx.undo?.()
  },
  {
    id: 'document.redo', get title() { return t('重做') }, key: 'Ctrl+Shift+Z', altKeys: ['Ctrl+Y'], scope: 'document', group: 'file',
    run: (ctx) => ctx.redo?.()
  },
  // ---- inline formatting --------------------------------------------------
  inline('format.bold', '加粗', 'Ctrl+B', INLINE_WRAPPERS.bold, ['bold', 'strong']),
  inline('format.italic', '斜体', 'Ctrl+I', INLINE_WRAPPERS.italic, ['italic', 'em']),
  inline('format.underline', '下划线', 'Ctrl+U', INLINE_WRAPPERS.underline, ['underline']),
  inline('format.strike', '删除线', 'Ctrl+Alt+S', INLINE_WRAPPERS.strike, ['strikethrough', 'del']),
  inline('format.code', '行内代码', 'Ctrl+E', INLINE_WRAPPERS.code, ['code', 'inline']),
  inline('format.mark', '高亮', 'Ctrl+Shift+H', INLINE_WRAPPERS.mark, ['mark', 'highlight']),

  // ---- block conversion ---------------------------------------------------
  ...LINE_PREFIXES.map((t, i) =>
    prefix(t, ['Ctrl+Shift+Digit8', 'Ctrl+Shift+Digit7', 'Ctrl+Shift+Digit9', 'Ctrl+Shift+U'][i])
  ),
  heading(1),
  heading(2),
  heading(3),
  heading(4),
  heading(5),
  heading(6),
  heading(0),

  // ---- insert -------------------------------------------------------------
  {
    id: 'insert.link',
    get title() { return t('链接') },
    keywords: ['link', 'url'],
    scope: 'document',
    key: 'Ctrl+K',
    group: 'insert',
    run: (ctx) => {
      const { start, end } = ctx.selection ?? { start: ctx.cursor, end: ctx.cursor }
      const label = ctx.source.slice(start, end)
      const text = `${ctx.source.slice(0, start)}[${label}](url)${ctx.source.slice(end)}`
      const urlStart = start + label.length + 3
      ctx.replace(0, ctx.source.length, text, urlStart)
      ctx.select(urlStart, urlStart + 3)
    }
  },
  {
    id: 'insert.image',
    get title() { return t('插入图片') },
    keywords: ['image', 'picture', 'paste'],
    scope: 'document',
    key: 'Ctrl+Shift+I',
    group: 'insert',
    run: (ctx) => {
      void (async () => {
        if (!ctx.docPath) {
          ctx.toast(t('请先保存文档，再插入图片；图片会放在文档旁边。'), 'error')
          return
        }
        const paths = await window.mdview.dialog.openImages()
        if (!paths || paths.length === 0) return
        const inserted = await window.mdview.asset.fromFiles(ctx.docPath, paths)
        if (inserted.length === 0) return
        const lines = inserted.map((a) => `![${altTextFor(paths[0])}](${a.relPath})`)
        // One image goes inline at the cursor; several get their own lines so
        // they render as stacked figures rather than one paragraph.
        const text = lines.length === 1 ? lines[0] : `\n\n${lines.join('\n')}\n\n`
        ctx.replace(ctx.cursor, ctx.cursor, text, ctx.cursor + text.length)
      })()
    }
  },
  {
    id: 'insert.codeblock',
    get title() { return t('代码块') },
    keywords: ['code', 'fence'],
    scope: 'document',
    key: 'Ctrl+Shift+C',
    group: 'insert',
    run: (ctx) => {
      const res = insertCodeBlock(ctx.source, ctx.cursor)
      ctx.replace(0, ctx.source.length, res.text, res.cursor)
    }
  },
  {
    id: 'insert.table',
    get title() { return t('插入表格') },
    keywords: ['table', 'grid'],
    scope: 'document',
    key: 'Ctrl+Alt+T',
    group: 'insert',
    // The size picker is a UI concern; 3×3 is the sane default for a shortcut
    // and the palette offers the full picker.
    run: (ctx) => {
      const res = insertTable(ctx.source, ctx.cursor, 3, 3)
      ctx.replace(0, ctx.source.length, res.text, res.cursor)
    }
  },
  {
    id: 'insert.footnote',
    get title() { return t('插入脚注') },
    keywords: ['footnote', 'note'],
    scope: 'document',
    key: 'Ctrl+Shift+W',
    group: 'insert',
    run: (ctx) => ctx.insertBlock('[^1]\n\n[^1]: ')
  },
  {
    id: 'insert.math',
    get title() { return t('插入数学块') },
    keywords: ['math', 'latex', 'katex', 'formula'],
    scope: 'document',
    key: 'Ctrl+Alt+M',
    group: 'insert',
    run: (ctx) => {
      const res = insertBlock(ctx.source, ctx.cursor, '$$\n\n$$')
      ctx.replace(0, ctx.source.length, res.text, res.cursor - 3)
    }
  },
  {
    id: 'insert.mermaid',
    get title() { return t('插入 Mermaid 图') },
    keywords: ['mermaid', 'diagram', 'chart', 'flowchart'],
    scope: 'document',
    key: 'Ctrl+Alt+K',
    group: 'insert',
    run: (ctx) => {
      const res = insertBlock(ctx.source, ctx.cursor, t('```mermaid\ngraph TD\n  A[开始] --> B[结束]\n```'))
      ctx.replace(0, ctx.source.length, res.text, res.cursor)
    }
  },
  {
    id: 'insert.toc',
    get title() { return t('插入目录') },
    keywords: ['toc', 'outline', 'contents'],
    scope: 'document',
    key: 'Ctrl+Alt+O',
    group: 'insert',
    run: (ctx) => ctx.insertBlock('[TOC]')
  },
  {
    id: 'insert.hr',
    get title() { return t('插入分隔线') },
    keywords: ['hr', 'divider', 'rule', 'separator'],
    scope: 'document',
    key: 'Ctrl+Alt+H',
    group: 'insert',
    run: (ctx) => ctx.insertBlock('---')
  },
  {
    id: 'insert.details',
    get title() { return t('插入折叠块') },
    keywords: ['details', 'collapse', 'fold'],
    scope: 'document',
    group: 'insert',
    run: (ctx) =>
      ctx.insertBlock(t('<details>\n<summary>展开</summary>\n\n\n</details>'))
  },
  {
    id: 'insert.pagebreak',
    get title() { return t('插入分页符') },
    keywords: ['page break', 'pdf'],
    scope: 'document',
    group: 'insert',
    run: (ctx) => ctx.insertBlock('<div style="page-break-after: always"></div>')
  },

  // ---- clipboard / find ---------------------------------------------------
  {
    id: 'find.open', get title() { return t('文件内查找') }, key: 'Ctrl+F',
    scope: 'document', group: 'find', keywords: ['find', 'search'],
    run: (ctx) => ctx.search?.('open')
  },
  {
    id: 'find.replace', get title() { return t('文件内替换') }, key: 'Ctrl+H',
    scope: 'document', group: 'find', keywords: ['replace'],
    enabled: () => !settingsSnapshot().readOnly,
    run: (ctx) => ctx.search?.('replace')
  },
  {
    id: 'find.next', get title() { return t('下一个匹配') }, key: 'F3',
    scope: 'document', group: 'find', keywords: ['find', 'next'],
    run: (ctx) => ctx.search?.('next')
  },
  {
    id: 'find.previous', get title() { return t('上一个匹配') }, key: 'Shift+F3',
    scope: 'document', group: 'find', keywords: ['find', 'previous'],
    run: (ctx) => ctx.search?.('previous')
  },
  {
    id: 'find.close', get title() { return t('关闭查找') }, key: 'Escape',
    scope: 'document', group: 'find',
    enabled: (ctx) => ctx.searchOpen?.() ?? false,
    run: (ctx) => ctx.search?.('close')
  },
  {
    id: 'clipboard.copyPlain',
    get title() { return t('复制为纯文本') },
    keywords: ['copy', 'plain'],
    scope: 'document',
    key: 'Ctrl+Shift+Alt+C',
    group: 'clipboard',
    run: (ctx) => {
      const { start, end } = ctx.selection ?? { start: 0, end: ctx.source.length }
      void window.mdview.clipboard.writeText(toPlainText(ctx.source.slice(start, end)))
      ctx.toast(t('已复制为纯文本'), 'success')
    }
  },
  {
    id: 'clipboard.copyAll',
    get title() { return t('复制全文') },
    scope: 'document',
    group: 'clipboard',
    run: (ctx) => void window.mdview.clipboard.writeText(ctx.source)
  },
  {
    id: 'find.selection',
    get title() { return t('查找选中内容') },
    keywords: ['find', 'search'],
    scope: 'find',
    key: 'Ctrl+Shift+F',
    group: 'find',
    run: (ctx) => {
      if (!ctx.selection) return
      const { start, end } = ctx.selection
      const text = ctx.source.slice(start, end)
      const next = ctx.source.indexOf(text, end)
      const at = next >= 0 ? next : ctx.source.indexOf(text)
      ctx.select(at, at + text.length)
      if (at === start) ctx.toast(t('没有其他匹配内容'))
    }
  },
  {
    id: 'view.toggleTheme',
    get title() { return t('切换深浅主题') },
    keywords: ['theme', 'dark', 'light'],
    scope: 'app',
    key: 'Ctrl+Shift+T',
    group: 'view',
    run: (ctx) => {
      const next = settingsSnapshot().theme === 'light' ? 'dark' : 'light'
      void patchSettings({ theme: next })
      ctx.toast(next === 'dark' ? t('已切换到深色') : t('已切换到浅色'))
    }
  },
  {
    id: 'view.toggleSidebar',
    get title() { return t('显示 / 隐藏侧栏') },
    keywords: ['sidebar', 'panel'],
    scope: 'app',
    key: 'Ctrl+Backslash',
    group: 'view',
    run: () => {
      void patchSettings({ sidebarVisible: !settingsSnapshot().sidebarVisible })
    }
  },
  {
    id: 'view.sourceMode',
    get title() { return t('切换源码模式') },
    keywords: ['source', 'raw', 'markdown', '源码'],
    scope: 'app',
    key: 'Ctrl+Slash',
    group: 'view',
    run: (ctx) => {
      const sourceMode = !settingsSnapshot().sourceMode
      void patchSettings({ sourceMode })
      ctx.toast(sourceMode ? t('已切换到源码模式') : t('已切换到排版编辑'))
    }
  },
  zoom('view.zoomIn', '放大正文', 'Ctrl+Equal', (size) => size + 1),
  zoom('view.zoomOut', '缩小正文', 'Ctrl+Minus', (size) => size - 1),
  zoom('view.zoomReset', '恢复默认字号', 'Ctrl+Digit0', () => DEFAULT_SETTINGS.fontSize),
  {
    id: 'view.showOutline', get title() { return t('大纲') }, keywords: ['outline', 'heading', '标题层级'],
    scope: 'app', group: 'view',
    run: () => patchSettings({ sidebarVisible: true, outlineVisible: true })
  },
  {
    id: 'view.showFiles', get title() { return t('文件') }, keywords: ['files', 'tree', '文件导航'],
    scope: 'app', group: 'view',
    run: () => patchSettings({ sidebarVisible: true, outlineVisible: false })
  },

  // ---- document hygiene ---------------------------------------------------
  {
    id: 'document.tidy',
    get title() { return t('格式化文档') },
    keywords: ['format', 'tidy', 'clean', 'normalise'],
    scope: 'document',
    key: 'Ctrl+Alt+F',
    group: 'file',
    run: (ctx) => {
      const next = tidyMarkdown(ctx.source)
      if (next === ctx.source) {
        ctx.toast(t('文档已是规范格式'))
        return
      }
      ctx.replace(0, ctx.source.length, next, Math.min(ctx.cursor, next.length))
    }
  },

  // ---- table: rows --------------------------------------------------------
  tableAction('table.row.insertAbove', '上方插入行', 'Ctrl+Enter', 'table-row', ['row', 'insert']),
  tableAction('table.row.insertBelow', '下方插入行', 'Ctrl+Alt+N', 'table-row'),
  tableAction('table.row.delete', '删除本行', 'Ctrl+Shift+Backspace', 'table-row', ['delete', 'row']),
  tableAction('table.row.moveUp', '上移本行', 'Alt+ArrowUp', 'table-row'),
  tableAction('table.row.moveDown', '下移本行', 'Alt+ArrowDown', 'table-row'),
  tableAction('table.row.copy', '复制本行', 'Ctrl+Shift+D', 'table-row'),
  tableAction('table.row.cut', '剪切本行', 'Ctrl+Shift+X', 'table-row'),

  // ---- table: columns -----------------------------------------------------
  tableAction('table.col.insertLeft', '左侧插入列', 'Ctrl+Shift+Enter', 'table-col'),
  tableAction('table.col.insertRight', '右侧插入列', 'Ctrl+Alt+Enter', 'table-col'),
  tableAction('table.col.delete', '删除本列', 'Ctrl+Alt+Backspace', 'table-col'),
  tableAction('table.col.moveLeft', '左移本列', 'Alt+ArrowLeft', 'table-col'),
  tableAction('table.col.moveRight', '右移本列', 'Alt+ArrowRight', 'table-col'),
  tableAction('table.col.alignLeft', '本列左对齐', 'Ctrl+Alt+L', 'table-col'),
  tableAction('table.col.alignCenter', '本列居中', 'Ctrl+Alt+C', 'table-col'),
  tableAction('table.col.alignRight', '本列右对齐', 'Ctrl+Alt+R', 'table-col'),

  // ---- table: cells -------------------------------------------------------
  tableAction('table.cell.merge', '合并单元格', 'Ctrl+M', 'table-cell'),
  tableAction('table.cell.split', '拆分单元格', 'Ctrl+Shift+M', 'table-cell'),
  tableAction('table.cell.clear', '清空单元格', 'Ctrl+Delete', 'table-cell'),
  tableAction('table.cell.newline', '单元格内换行', 'Alt+Enter', 'table-cell'),

  // ---- table: data --------------------------------------------------------
  tableAction('table.sort.asc', '按本列升序', 'Ctrl+Alt+ArrowUp', 'table-data', ['sort']),
  tableAction('table.sort.desc', '按本列降序', 'Ctrl+Alt+ArrowDown', 'table-data'),
  tableAction('table.select.all', '选中整张表', 'Ctrl+Shift+Space', 'table-data'),
  tableAction('table.stats.sum', '本列求和', '', 'table-data'),
  tableAction('table.stats.avg', '本列均值', '', 'table-data'),
  tableAction('table.stats.count', '本列计数', '', 'table-data'),
  tableStub('table.stats.min', '本列最小值', '', 'table-data'),
  tableStub('table.stats.max', '本列最大值', '', 'table-data'),
  tableAction('table.paste.fromClipboard', '从剪贴板建表', 'Ctrl+Alt+V', 'table-data'),
  tableStub('table.paste.fromExcel', '从 Excel 粘贴', '', 'table-data'),

  // ---- table: styling -----------------------------------------------------
  tableAction('table.style.pad', '整表美化（对齐竖线）', 'Ctrl+Alt+P', 'table-style'),
  tableStub('table.style.zebra', '斑马纹', '', 'table-style'),
  tableStub('table.style.compact', '紧凑样式', '', 'table-style'),
  tableStub('table.style.borderless', '无边框', '', 'table-style'),
  tableStub('table.style.card', '卡片样式', '', 'table-style'),
  tableStub('table.style.center', '表格居中', '', 'table-style'),

  // ---- table: structure ---------------------------------------------------
  tableAction('table.struct.transpose', '转置表格', 'Ctrl+Alt+X', 'table-struct'),
  tableAction('table.struct.headerOn', '首行设为表头', '', 'table-struct'),
  tableAction('table.struct.headerOff', '取消表头', '', 'table-struct'),
  tableStub('table.struct.toCsv', '表格转 CSV', '', 'table-struct'),
  tableStub('table.struct.fromCsv', 'CSV 转表格', '', 'table-struct'),
  tableStub('table.struct.toJson', '表格转 JSON', '', 'table-struct'),
  tableStub('table.struct.toList', '表格转列表', '', 'table-struct'),
  tableStub('table.struct.exportCsv', '导出本表为 CSV', '', 'table-struct'),
  tableStub('table.struct.delete', '删除整张表格', '', 'table-struct'),

  // ---- code block ---------------------------------------------------------
  {
    id: 'code.copy',
    get title() { return t('复制代码') },
    keywords: ['copy', 'code'],
    scope: 'codeblock',
    key: 'Ctrl+Shift+Alt+E',
    group: 'code',
    run: (ctx) => {
      const block = codeAt(ctx.source, ctx.cursor)
      if (!block) return
      void window.mdview.clipboard.writeText(block.body)
      ctx.toast(t('已复制代码'), 'success')
    }
  },
  {
    id: 'code.selectAll',
    get title() { return t('全选代码块') },
    scope: 'codeblock',
    key: 'Ctrl+Shift+A',
    group: 'code',
    run: (ctx) => {
      const block = codeAt(ctx.source, ctx.cursor)
      if (!block) return
      ctx.select(block.bodyStart, block.bodyEnd)
    }
  },
  {
    id: 'code.unwrap',
    get title() { return t('解包为纯文本') },
    keywords: ['unwrap', 'strip', 'fence'],
    scope: 'codeblock',
    group: 'code',
    run: (ctx) => {
      const block = codeAt(ctx.source, ctx.cursor)
      if (!block) return
      ctx.replace(block.start, block.end, block.body, block.start)
    }
  },
  {
    id: 'code.toInline',
    get title() { return t('转换为行内代码') },
    scope: 'codeblock',
    group: 'code',
    run: (ctx) => {
      const block = codeAt(ctx.source, ctx.cursor)
      if (!block) return
      const inline = block.body.split('\n').join(' ')
      ctx.replace(block.start, block.end, `\`${inline}\``, block.start)
    }
  },
  {
    id: 'code.pickLanguage',
    get title() { return t('指定语言') },
    keywords: ['language', 'lang', 'syntax'],
    scope: 'codeblock',
    group: 'code',
    run: (ctx) => ctx.pickCodeLanguage?.()
  },

  // ---- image / link -------------------------------------------------------
  {
    id: 'image.fullscreen',
    get title() { return t('全屏查看') },
    keywords: ['zoom', 'preview'],
    scope: 'image',
    key: 'Z',
    group: 'image',
    run: (ctx) => ctx.fullscreenImage?.()
  },
  {
    id: 'image.copyPath',
    get title() { return t('复制图片路径') },
    scope: 'image',
    group: 'image',
    run: (ctx) => {
      void window.mdview.clipboard.writeText(relImageAt(ctx.source, ctx.cursor) ?? '')
      ctx.toast(t('路径已复制'), 'success')
    }
  },
  {
    id: 'image.reveal',
    get title() { return t('在文件管理器中显示') },
    scope: 'image',
    group: 'image',
    run: () => undefined
  },
  {
    id: 'image.delete',
    get title() { return t('删除图片') },
    scope: 'image',
    group: 'image',
    run: (ctx) => {
      const span = imageSpanAt(ctx.source, ctx.cursor)
      if (!span) return
      ctx.replace(span[0], span[1], '', span[0])
    }
  },
  {
    id: 'link.open',
    get title() { return t('打开链接') },
    scope: 'link',
    group: 'link',
    run: (ctx) => {
      const href = linkAt(ctx.source, ctx.cursor)
      if (href !== null) void followDocumentLink(ctx, href)
    }
  },
  {
    id: 'link.copy',
    get title() { return t('复制链接地址') },
    scope: 'link',
    group: 'link',
    run: (ctx) => {
      void window.mdview.clipboard.writeText(linkAt(ctx.source, ctx.cursor) ?? '')
      ctx.toast(t('链接已复制'), 'success')
    }
  },
  {
    id: 'link.remove',
    get title() { return t('移除链接（保留文字）') },
    scope: 'link',
    group: 'link',
    enabled: (ctx) => unlinkAt(ctx.source, ctx.cursor) !== null,
    run: (ctx) => {
      const link = unlinkAt(ctx.source, ctx.cursor)
      if (link) ctx.replace(link.from, link.to, link.text, link.from)
    }
  }
]

/* ---------------------------------------------------------------- selectors */

export function actionsIn(group: string): ActionDef[] {
  return ACTIONS.filter((a) => a.group === group)
}

export function findAction(id: string): ActionDef | undefined {
  return ACTIONS.find((a) => a.id === id)
}

/** Every binding in the registry, for the cheat-sheet overlay. */
export function allBindings(): { key: string; title: string; id: string }[] {
  return ACTIONS.filter((a) => a.key).map((a) => ({
    key: a.key as string,
    title: a.title,
    id: a.id
  }))
}

/* --------------------------------------------------------- source scanners */

export interface CodeBlockSpan {
  start: number
  end: number
  bodyStart: number
  bodyEnd: number
  lang: string
  body: string
}

/** The fenced block containing `offset`, if any. */
export function codeAt(src: string, offset: number): CodeBlockSpan | null {
  const re = /^```(\w*)[^\n]*\n([\s\S]*?)^```/gm
  for (const m of src.matchAll(re)) {
    const start = m.index ?? 0
    const end = start + m[0].length
    if (offset >= start && offset <= end) {
      const bodyStart = start + m[0].indexOf('\n') + 1
      return {
        start,
        end,
        bodyStart,
        bodyEnd: bodyStart + m[2].length,
        lang: m[1] ?? '',
        body: m[2]
      }
    }
  }
  return null
}

export function imageSpanAt(src: string, offset: number): [number, number] | null {
  const re = /!\[[^\]]*\]\([^)]*\)/g
  for (const m of src.matchAll(re)) {
    const start = m.index ?? 0
    if (offset >= start && offset <= start + m[0].length) return [start, start + m[0].length]
  }
  return null
}

export function relImageAt(src: string, offset: number): string | null {
  const span = imageSpanAt(src, offset)
  if (!span) return null
  return /!\[[^\]]*\]\(([^)]*)\)/.exec(src.slice(span[0], span[1]))?.[1] ?? null
}

export { linkAt }

/* -------------------------------------------------------- auto-pair bridge */

export { autoPair }
export type { ActionContext }
