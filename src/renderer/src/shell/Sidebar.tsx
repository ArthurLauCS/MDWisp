import { t } from '../i18n'
import { useState, type CSSProperties } from 'react'
import { useWorkspace } from '../state/workspace'
import { usePatchSettings, useSettings } from '../state/settings'
import { openDocument, useDocuments } from '../state/documents'
import { useEditorContext } from '../state/editor-context'
import { AssetPanel } from '../assets/AssetPanel'
import { Outline } from '../outline/Outline'
import { ResizeHandle } from './ResizeHandle'
import { SplitHandle, sidebarCollapse } from './SplitHandle'
import { findAction } from '../actions/registry'
import type { TreeNode } from '@shared/types'
import './sidebar.css'

function TreeRow({ node, depth }: { node: TreeNode; depth: number }): JSX.Element | null {
  const [open, setOpen] = useState(depth < 1)
  const { active } = useDocuments()

  if (node.isAssetDir) return null

  if (node.kind === 'dir') {
    return (
      <div className="tree__group">
        <button
          className="nav-row tree__row tree__row--dir"
          style={{ '--depth': depth } as CSSProperties}
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
        >
          <span className={`tree__chevron ${open ? 'is-open' : ''}`} aria-hidden />
          <span className="tree__label">{node.name}</span>
        </button>
        {open && node.children?.map((c) => <TreeRow key={c.path} node={c} depth={depth + 1} />)}
      </div>
    )
  }

  const isActive = active?.meta.path === node.path
  return (
    <button
      className={`nav-row tree__row tree__row--file ${isActive ? 'is-active' : ''}`}
      style={{ '--depth': depth } as CSSProperties}
      aria-current={isActive ? 'page' : undefined}
      onClick={() => void openDocument(node.path)}
      title={node.path}
    >
      <span className="tree__label">{node.name.replace(/\.(md|markdown|mdx)$/i, '')}</span>
    </button>
  )
}

export function Sidebar(): JSX.Element {
  const { info } = useWorkspace()
  const { active } = useDocuments()
  const { ctx } = useEditorContext()
  const patch = usePatchSettings()
  const settings = useSettings()
  const filesAction = findAction('view.showFiles')!
  const outlineAction = findAction('view.showOutline')!
  const isCurrent = ctx.docPath === (active?.meta.path || null)
  const source = active ? (isCurrent ? ctx.source : active.text) : ''
  const close = (): void => patch({ sidebarVisible: false })

  return (
    <>
      {/* Only reachable under the narrow breakpoint; a click anywhere off the
          panel dismisses it, which is what an overlay implies. */}
      <button className="sidebar__scrim" onClick={close} aria-label={t('关闭侧栏')} />
      <aside className="sidebar" aria-label={t('导航侧栏')} data-collapse={active?.meta.path ? sidebarCollapse(settings.assetsRatio) : ''}
        style={{ '--assets-ratio': settings.assetsRatio } as CSSProperties}>
      <ResizeHandle side="left" />
      <div className="side-panel__head sidebar__head">
        <div className="sidebar__tabs" aria-label={t('导航视图')}>
          <button className={`sidebar__tab ${!settings.outlineVisible ? 'is-active' : ''}`} aria-pressed={!settings.outlineVisible}
            onClick={() => void filesAction.run(ctx)}>{filesAction.title}</button>
          <button className={`sidebar__tab ${settings.outlineVisible ? 'is-active' : ''}`} aria-pressed={settings.outlineVisible}
            onClick={() => void outlineAction.run(ctx)}>{outlineAction.title}</button>
        </div>
        <div className="side-panel__subtitle" title={settings.outlineVisible ? active?.meta.path : info?.rootPath}>
          {settings.outlineVisible ? (active ? active.meta.path ? active.meta.stem : t('未命名') : t('未打开文档')) : info?.name ?? t('未打开目录')}
        </div>
      </div>
      {settings.outlineVisible ? <Outline key={active?.meta.id ?? 'empty'} source={source}
        cursor={isCurrent ? ctx.cursor : 0} onJump={(offset) => ctx.jump?.(offset)} /> : <nav className="sidebar__tree" aria-label={t('文件导航')}>
        {info ? (
          info.tree.children?.map((c) => <TreeRow key={c.path} node={c} depth={0} />)
        ) : (
          <p className="sidebar__empty">
            {t('还没有打开目录。')}<br />
            {t('用上方「打开目录」选一个装满 markdown 的文件夹。')}</p>
        )}
      </nav>}
      {active?.meta.path ? (<>
        <SplitHandle />
        <AssetPanel
          key={active.meta.path}
          docPath={active.meta.path}
          source={source}
        />
      </>) : <div className="sidebar__draft-note">{t('文字可直接输入。保存文档后即可插入本地图片，并启用自动保存与历史记录。')}</div>}
      </aside>
    </>
  )
}
