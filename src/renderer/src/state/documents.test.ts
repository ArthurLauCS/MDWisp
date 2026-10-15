import { beforeEach, describe, expect, it, vi } from 'vitest'

const meta = (path: string) => ({ id: path, path, stem: path.split('/').pop()!, parentDir: '/notes', assetDir: '/notes/img', layout: 'flat' as const })
let documents: typeof import('./documents')
let bridge: any

beforeEach(async () => {
  vi.resetModules()
  bridge = {
    doc: {
      read: vi.fn(async (path: string) => ({ meta: meta(path), text: '# Saved', body: '# Saved', frontmatter: null, conflictWithDisk: false })),
      write: vi.fn(async (path: string) => meta(path))
    },
    dialog: { saveFile: vi.fn(async () => '/notes/draft.md'), confirmSave: vi.fn(async () => 'cancel'), openDocument: vi.fn(async () => null) },
    window: { open: vi.fn(async () => undefined), close: vi.fn() },
    history: { record: vi.fn(async () => null) }
  }
  vi.stubGlobal('window', { mdview: bridge })
  documents = await import('./documents')
})

describe('document lifecycle', () => {
  it('opens new windows without replacing or prompting for the current draft', async () => {
    await documents.initializeDocument()
    documents.updateDocumentBuffer('unsaved work')
    await documents.newDocument()
    await documents.openDocument('/notes/other.md', 'heading')
    expect(bridge.window.open.mock.calls).toEqual([[null], ['/notes/other.md', 'heading']])
    expect(documents.documentBuffer()).toBe('unsaved work')
    expect(documents.hasUnsavedChanges()).toBe(true)
    expect(bridge.dialog.confirmSave).not.toHaveBeenCalled()
    documents.closeDocument()
    expect(bridge.window.close).toHaveBeenCalledOnce()
  })

  it('keeps a new draft in memory until the first explicit save', async () => {
    await documents.initializeDocument()
    documents.updateDocumentBuffer('草稿 English\n第二行')
    expect(documents.currentDocument()?.meta.path).toBe('')
    expect(bridge.doc.write).not.toHaveBeenCalled()
    expect(bridge.history.record).not.toHaveBeenCalled()
    expect(await documents.saveDocument()).toBe(true)
    expect(bridge.doc.write).toHaveBeenCalledWith('/notes/draft.md', '草稿 English\n第二行', undefined)
    expect(documents.currentDocument()?.meta.path).toBe('/notes/draft.md')
    expect(documents.hasUnsavedChanges()).toBe(false)
    documents.updateDocumentBuffer('changed')
    await documents.saveDocument(false)
    expect(bridge.dialog.saveFile).toHaveBeenCalledTimes(1)
  })

  it('preserves a draft when save is cancelled or disk writing fails', async () => {
    await documents.initializeDocument()
    documents.updateDocumentBuffer('keep me')
    bridge.dialog.saveFile.mockResolvedValueOnce(null)
    expect(await documents.saveDocument()).toBe(false)
    expect(documents.documentBuffer()).toBe('keep me')
    expect(bridge.doc.write).not.toHaveBeenCalled()
    bridge.doc.write.mockRejectedValueOnce(new Error('disk full'))
    expect(await documents.saveDocument()).toBe(false)
    expect(documents.currentDocument()?.meta.path).toBe('')
    expect(documents.hasUnsavedChanges()).toBe(true)
  })

  it('honors cancel, save and discard on document switches', async () => {
    await documents.initializeDocument()
    documents.updateDocumentBuffer('keep')
    await documents.loadDocument('/notes/other.md')
    expect(documents.documentBuffer()).toBe('keep')
    bridge.dialog.confirmSave.mockResolvedValueOnce('save')
    bridge.dialog.saveFile.mockResolvedValueOnce(null)
    await documents.initializeDocument()
    expect(documents.documentBuffer()).toBe('keep')
    bridge.dialog.confirmSave.mockResolvedValueOnce('save')
    await documents.loadDocument('/notes/other.md')
    expect(bridge.doc.write).toHaveBeenCalledWith('/notes/draft.md', 'keep', undefined)
    expect(documents.documentBuffer()).toBe('# Saved')
    documents.updateDocumentBuffer('discard')
    bridge.dialog.confirmSave.mockResolvedValueOnce('discard')
    await documents.initializeDocument()
    expect(documents.documentBuffer()).toBe('')
    expect(bridge.doc.write).toHaveBeenCalledTimes(1)
  })

  it('does not clear newer input or open a second save dialog during a write', async () => {
    await documents.initializeDocument()
    documents.updateDocumentBuffer('first')
    let finish!: (value: ReturnType<typeof meta>) => void
    bridge.doc.write.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve }))
    const saving = documents.saveDocument()
    expect(documents.saveDocument()).toBe(saving)
    await vi.waitFor(() => expect(bridge.doc.write).toHaveBeenCalled())
    documents.updateDocumentBuffer('newer')
    finish(meta('/notes/draft.md'))
    await saving
    expect(documents.documentBuffer()).toBe('newer')
    expect(documents.hasUnsavedChanges()).toBe(true)
    expect(bridge.dialog.saveFile).toHaveBeenCalledTimes(1)
  })

  it('retains the current file when opening fails and saves its complete frontmatter', async () => {
    const text = '---\ntitle: metadata\n---\n# Body'
    bridge.doc.read.mockResolvedValueOnce({ meta: meta('/notes/a.md'), text, body: '# Body', frontmatter: { title: 'metadata' }, conflictWithDisk: false })
    await documents.loadDocument('/notes/a.md')
    await documents.saveDocument()
    expect(bridge.doc.write).toHaveBeenCalledWith('/notes/a.md', text, text)
    bridge.doc.read.mockRejectedValueOnce(new Error('file missing'))
    await documents.loadDocument('/notes/missing.md')
    expect(documents.currentDocument()?.meta.path).toBe('/notes/a.md')
  })

  it('takes edits from other programs as the new save baseline', async () => {
    await documents.loadDocument('/notes/a.md')
    expect(await documents.diskState()).toMatchObject({ changed: false })
    const external = { meta: meta('/notes/a.md'), text: '# Agent', body: '# Agent', frontmatter: null, conflictWithDisk: false }
    bridge.doc.read.mockResolvedValue(external)
    expect(await documents.diskState()).toEqual({ text: '# Agent', changed: true, dirty: false })
    documents.updateDocumentBuffer('# Mine')
    expect(await documents.diskState()).toMatchObject({ changed: true, dirty: true })
    documents.adoptDiskText('# Agent')
    expect(documents.hasUnsavedChanges()).toBe(true)
    documents.updateDocumentBuffer('# Agent')
    expect(documents.hasUnsavedChanges()).toBe(false)
    documents.updateDocumentBuffer('# Agent, then mine')
    await documents.saveDocument()
    expect(bridge.doc.write).toHaveBeenLastCalledWith('/notes/a.md', '# Agent, then mine', '# Agent')
  })

  it('treats its own save as already seen', async () => {
    await documents.loadDocument('/notes/a.md')
    documents.updateDocumentBuffer('# Written here')
    const saving = documents.saveDocument()
    bridge.doc.read.mockResolvedValue({ meta: meta('/notes/a.md'), text: '# Written here', body: '', frontmatter: null, conflictWithDisk: false })
    expect(await documents.diskState()).toMatchObject({ changed: false, dirty: false })
    await saving
  })
})
