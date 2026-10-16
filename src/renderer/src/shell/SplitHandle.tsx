import { t } from '../i18n'
import { useEffect, useRef } from 'react'
import { DEFAULT_SETTINGS } from '@shared/types'
import { usePatchSettings, useSettings } from '../state/settings'

// Closer than this to an edge, the part being squeezed hides instead of shrinking.
const SNAP = 64

/** Splits the sidebar between navigation and images; ratio is the images' share of the sidebar height. */
export function SplitHandle(): JSX.Element {
  const settings = useSettings()
  const patch = usePatchSettings()
  const drag = useRef<{ top: number; bottom: number; next: number; element: HTMLElement } | null>(null)
  const frame = useRef<number | null>(null)

  useEffect(() => () => {
    if (frame.current !== null) cancelAnimationFrame(frame.current)
    if (drag.current) delete document.documentElement.dataset.resizing
  }, [])

  return <div className="split-resizer" role="separator" tabIndex={0}
    aria-label={t('调整大纲与图片高度')} aria-orientation="horizontal"
    aria-valuenow={Math.round(settings.assetsRatio * 100)} aria-valuemin={0} aria-valuemax={100}
    onPointerDown={(event) => {
      if (event.button !== 0) return
      event.preventDefault()
      event.currentTarget.focus()
      event.currentTarget.setPointerCapture(event.pointerId)
      const element = event.currentTarget.parentElement!
      const head = element.querySelector('.sidebar__head')!.getBoundingClientRect()
      drag.current = { top: head.bottom, bottom: element.getBoundingClientRect().bottom, next: settings.assetsRatio, element }
      document.documentElement.dataset.resizing = 'row'
    }}
    onPointerMove={(event) => {
      const d = drag.current
      if (!d) return
      const y = event.clientY
      // Dragging up past the top hides the navigation, down past the bottom hides the images.
      d.next = y - d.top < SNAP ? 1 : d.bottom - y < SNAP ? 0
        : (d.bottom - y) / d.element.getBoundingClientRect().height
      event.currentTarget.setAttribute('aria-valuenow', String(Math.round(d.next * 100)))
      if (frame.current === null) frame.current = requestAnimationFrame(() => {
        frame.current = null
        if (!drag.current) return
        drag.current.element.style.setProperty('--assets-ratio', String(drag.current.next))
        drag.current.element.dataset.collapse = sidebarCollapse(drag.current.next)
      })
    }}
    onPointerUp={(event) => event.currentTarget.releasePointerCapture(event.pointerId)}
    onLostPointerCapture={() => {
      if (!drag.current) return
      if (frame.current !== null) cancelAnimationFrame(frame.current)
      frame.current = null
      delete document.documentElement.dataset.resizing
      patch({ assetsRatio: drag.current.next })
      drag.current = null
    }}
    onDoubleClick={() => patch({ assetsRatio: DEFAULT_SETTINGS.assetsRatio })}
    onKeyDown={(event) => {
      if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
      event.preventDefault()
      patch({ assetsRatio: Math.max(0, Math.min(1, Math.round(settings.assetsRatio * 20 + (event.key === 'ArrowUp' ? 1 : -1)) / 20)) })
    }} />
}

export const sidebarCollapse = (ratio: number): string => ratio === 0 ? 'assets' : ratio === 1 ? 'nav' : ''
