/** Real Electron check: front matter display and every export mode writes a real file. Isolated profile. */
const { app, BrowserWindow, dialog } = require('electron')
const path = require('node:path')
const fs = require('node:fs/promises')
const assert = require('node:assert/strict')
const OUT = path.resolve(process.env.MDVIEW_CHECK_OUT || process.argv[2] || 'design-review/export-check')
const ROOT = process.env.MDVIEW_CHECK_ROOT || path.resolve(__dirname, '..')
const saves = []
dialog.showSaveDialog = async () => ({ canceled: false, filePath: saves.shift() })
require('node:fs').mkdirSync(path.join(OUT, 'documents'), { recursive: true })
app.setPath('userData', path.join(OUT, 'profile'))
app.setPath('documents', path.join(OUT, 'documents'))
require(process.env.MDVIEW_PACKAGED_ENTRY || path.join(ROOT, 'out/main/index.js'))
const wait = (ms) => new Promise((r) => setTimeout(r, ms))

async function main() {
  const docDir = path.join(OUT, 'workspace', '欢迎使用')
  await fs.cp(path.join(app.isPackaged ? process.resourcesPath : ROOT, 'stock', '欢迎使用'), docDir, { recursive: true })
  await fs.appendFile(path.join(docDir, '欢迎使用.md'), '\n\n$$\nE = mc^2\n$$\n')
  for (let i = 0; i < 60 && !BrowserWindow.getAllWindows().length; i++) await wait(100)
  const win = BrowserWindow.getAllWindows()[0]
  win.setSize(1440, 900)
  const run = (code) => win.webContents.executeJavaScript(code, true)
  const shot = async (name) => {
    if (win.isMinimized()) win.restore()
    win.showInactive()
    await wait(250)
    await fs.writeFile(path.join(OUT, `${name}.png`), (await win.webContents.capturePage()).toPNG())
  }
  await wait(1200)
  await run(`window.__mdview.setSettings({ language: 'zh-CN', theme: 'dark', sidebarVisible: true, autoSave: false, motion: 'off' })`)
  await run(`window.__mdview.loadDocument(${JSON.stringify(path.join(docDir, '欢迎使用.md'))})`)
  await wait(800)

  assert.equal(await run(`document.querySelectorAll('.live-frontmatter').length`), 5)
  assert.equal(await run(`document.querySelector('.live-heading').textContent`), '图片应该跟着文档走')
  assert.equal(await run(`!!Array.from(document.querySelectorAll('.sidebar *')).find(el => el.textContent.startsWith('title:'))`), false)
  await shot('01-frontmatter')

  run(`window.__mdview.openPanel('export')`)
  await wait(500)
  const magic = { md: null, zip: 'PK', html: '<!doctype html>', pdf: '%PDF' }
  for (const [index, ext] of Object.keys(magic).entries()) {
    const target = path.join(OUT, 'documents', `out.${ext}`)
    saves.push(target)
    await run(`document.querySelectorAll('.mode')[${index}].click()`)
    await wait(1200)
    await shot(`02-export-${ext}`)
    await run(`document.querySelector('.panel__foot .btn--primary').click()`)
    for (let i = 0; i < 100 && !(await run(`document.querySelector('.panel__foot').textContent.includes('已导出到')`)); i++) await wait(100)
    const data = await fs.readFile(target)
    assert.ok(data.length > 500, `${ext} is empty`)
    if (magic[ext]) assert.ok(data.subarray(0, 20).toString().startsWith(magic[ext]), `${ext} has wrong content`)
    console.log('PASS', ext, data.length, 'bytes')
  }
  assert.doesNotMatch(await fs.readFile(path.join(OUT, 'documents', 'out.html'), 'utf8'), /title: 欢迎使用/)
  const exportedHtml = await fs.readFile(path.join(OUT, 'documents', 'out.html'), 'utf8')
  assert.match(exportedHtml, /katex-display/)
  assert.match(exportedHtml, /data:font\/woff2;base64,/)
  assert.doesNotMatch(exportedHtml, /url\(fonts\//)
  const htmlWindow = new BrowserWindow({ show: false, width: 1280, height: 960, webPreferences: { sandbox: true, nodeIntegration: false } })
  try {
    await htmlWindow.loadFile(path.join(OUT, 'documents', 'out.html'))
    const layout = await htmlWindow.webContents.executeJavaScript(`document.fonts.ready.then(() => {
      const article = document.querySelector('article'), style = getComputedStyle(article)
      return { maxWidth: style.maxWidth, padding: parseFloat(style.paddingLeft),
        fonts: [...document.fonts].filter(font => font.status === 'loaded').map(font => font.family),
        images: [...document.images].every(img => img.complete && img.naturalWidth > 0) }
    })`)
    assert.notEqual(layout.maxWidth, 'none', 'standalone article styles must apply')
    assert.ok(layout.padding > 0 && layout.images)
    assert.ok(layout.fonts.includes('MDView Rounded'))
    await fs.writeFile(path.join(OUT, 'html-render.png'), (await htmlWindow.webContents.capturePage()).toPNG())
    console.log('PASS standalone HTML layout, fonts and images')
  } finally { htmlWindow.destroy() }
  if (process.env.MDVIEW_PACKAGED_ENTRY) {
    assert.equal(app.isPackaged, true)
    await run('window.__mdview.closePanel()')
    await wait(100)
    await run(`document.querySelector('button[title="使用说明"]').click()`)
    await wait(200)
    const project = path.join(OUT, 'plugin-project')
    await fs.mkdir(project, { recursive: true })
    dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [project] })
    await run(`Array.from(document.querySelectorAll('button')).find(el => el.textContent === '安装到项目').click()`)
    for (let i = 0; i < 50 && !(await run(`document.querySelector('[role="status"]')?.textContent.includes('已安装到')`)); i++) await wait(100)
    assert.match(await run(`document.querySelector('[role="status"]').textContent`), /已安装到/)
    for (const name of ['doc', 'share']) {
      assert.match(await fs.readFile(path.join(project, '.agents/skills', `mdwisp-${name}/SKILL.md`), 'utf8'), new RegExp(`name: mdwisp-${name}`))
    }
    await run(`document.querySelector('[role="status"]').scrollIntoView({block:'center'})`)
    await shot('03-portable-skills-installed')
    console.log('PASS packaged app, bundled CSS/fonts, HTML/PDF and Codex/Cursor installation')
  }
}

app.whenReady().then(main).then(() => app.exit(0), (error) => { console.error(error); app.exit(1) })
