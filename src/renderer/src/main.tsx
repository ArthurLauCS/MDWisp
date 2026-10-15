import React from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App'
import { loadDocument, openDocument } from './state/documents'
import { patchSettings } from './state/settings'
import { editorContext } from './state/editor-context'
import './styles/tokens.css'
import './styles/fonts.css'
import './styles/base.css'
import './styles/markdown.css'
import 'katex/dist/katex.min.css'
import './App.css'

const el = document.getElementById('root')
if (!el) throw new Error('#root missing from index.html')

/**
 * Drive the UI from outside the window. The screenshot harness and future
 * end-to-end tests need to reach the same state layer the UI uses — poking
 * `window.mdview` directly would bypass the stores and leave the view stale.
 * Deliberately small: documents, settings, panels, and nothing else.
 */
declare global {
  interface Window {
    __mdview?: {
      openWorkspace: (path: string) => Promise<void>
      openDocument: (path: string) => Promise<void>
      loadDocument: (path: string) => Promise<void>
      setSettings: (patch: Record<string, unknown>) => Promise<void>
      openPanel: (id: 'settings' | 'export' | 'shortcuts' | 'palette') => void
      closePanel: () => void
      editorContext: typeof editorContext
    }
  }
}

window.__mdview = {
  editorContext,
  openWorkspace: async (path) => {
    const { openWorkspacePath } = await import('./state/workspace')
    await openWorkspacePath(path)
  },
  openDocument,
  loadDocument,
  setSettings: async (patch) => {
    await patchSettings(patch as never)
  },
  openPanel: (id) => {
    void import('./state/ui').then(({ openPanel }) => openPanel(id))
  },
  closePanel: () => {
    void import('./state/ui').then(({ closePanel }) => closePanel())
  }
}

createRoot(el).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
)
