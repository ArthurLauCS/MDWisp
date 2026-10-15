/** Exercise advertised shortcuts through Electron events, or Windows keys with --native. */
const { app, BrowserWindow, dialog, clipboard } = require('electron')
const path = require('node:path')
const fs = require('node:fs/promises')
const assert = require('node:assert/strict')
const OUT = path.resolve(process.argv[2] || 'design-review/shortcuts-layout')
require('node:fs').mkdirSync(path.join(OUT, 'documents'), { recursive: true })
app.setPath('userData', path.join(OUT, 'profile'))
app.setPath('documents', path.join(OUT, 'documents'))
const saves = [], opens = []
const originalClipboard = { text: clipboard.readText(), html: clipboard.readHTML(), rtf: clipboard.readRTF(), image: clipboard.readImage() }
dialog.showSaveDialog = async () => { assert.ok(saves.length, 'unexpected save'); return { canceled: false, filePath: saves.shift() } }
dialog.showOpenDialog = async () => { assert.ok(opens.length, 'unexpected open'); return { canceled: false, filePaths: [opens.shift()] } }
dialog.showMessageBox = async () => ({ response: 1 })
require(path.resolve(__dirname, '../out/main/index.js'))
const wait = ms => new Promise(resolve => setTimeout(resolve, ms))
const used = new Set()
let sequence = 0
const canonical = binding => binding.replace(/Digit/g, '').replace(/ArrowUp/g, '↑').replace(/ArrowDown/g, '↓')
  .replace(/ArrowLeft/g, '←').replace(/ArrowRight/g, '→').replace(/Backspace/g, '⌫').replace(/Enter/g, '↵')
  .replace(/Space/g, '空格').replace(/Backslash/g, '\\').replace(/Slash/g, '/').replace(/Equal/g, '=').replace(/Minus/g, '-')
  .replace(/Escape/g, 'Esc').replace(/\s*\+\s*/g, '+')

async function main() {
  for (let i = 0; i < 80 && !BrowserWindow.getAllWindows().length; i++) await wait(100)
  let win = BrowserWindow.getAllWindows()[0]
  win.setTitle('MDWisp — Shortcut verification')
  win.setSize(1440, 900)
  const run = code => win.webContents.executeJavaScript(code, true)
  const ctx = 'window.__mdview.editorContext()'
  const until = async code => {
    for (let i = 0; i < 70; i++) { if (await run(code)) return; await wait(60) }
    throw new Error(`Timed out: ${code}`)
  }
  const press = async binding => {
    if (process.argv.includes('--native')) {
      const request = { id: ++sequence, binding }
      await fs.writeFile(path.join(OUT, 'key-request.json'), JSON.stringify(request))
      let acknowledged = false
      for (let i = 0; i < 1800; i++) {
        const ack = await fs.readFile(path.join(OUT, 'key-ack.txt'), 'utf8').catch(() => '')
        if (ack === String(request.id)) { acknowledged = true; break }
        await wait(100)
      }
      assert.ok(acknowledged, `Windows key timed out: ${binding}`)
      used.add(canonical(binding))
      await wait(120)
      await fs.appendFile(path.join(OUT, 'key-results.jsonl'), JSON.stringify({ ...request, state: await run(`({event:window.__lastKey,source:${ctx}.source,selection:${ctx}.selection})`) }) + '\n')
      return
    }
    const parts = binding.split('+')
    const keyCode = parts.pop().replace(/^Digit/, '').replace(/^Arrow/, '').replace('Backslash', '\\').replace('Space', ' ')
      .replace('Slash', '/').replace('Equal', '=').replace('Minus', '-')
    const modifiers = parts.map(p => ({ Ctrl: 'control', Shift: 'shift', Alt: 'alt' })[p])
    win.webContents.sendInputEvent({ type: 'keyDown', keyCode, modifiers })
    win.webContents.sendInputEvent({ type: 'keyUp', keyCode, modifiers })
    used.add(canonical(binding))
    await wait(120)
  }
  const edit = async (source, start = source.length, end = start) => {
    await run(`${ctx}.replace(0, ${ctx}.source.length, ${JSON.stringify(source)}); ${ctx}.select(${start}, ${end})`)
    await wait(100)
  }
  const text = () => run(`${ctx}.source`)
  const shot = async name => {
    await wait(250)
    await fs.writeFile(path.join(OUT, name + '.png'), (await win.webContents.capturePage()).toPNG())
    console.log('wrote', name)
  }
  await until(`!!document.querySelector('.cm-content')`)
  await run('document.fonts.ready.then(() => true)')
  await run(`window.addEventListener('keydown', e => { window.__lastKey = {key:e.key,code:e.code,ctrl:e.ctrlKey,alt:e.altKey,shift:e.shiftKey} }, true)`)
  await run(`window.__mdview.setSettings({language:'zh-CN',autoSave:false,historyEnabled:false,autoPair:false,motion:'off'})`)
  assert.equal(win.isMenuBarVisible(), false, 'native menu must not steal editor shortcuts')

  await press('F1')
  await until(`!!document.querySelector('.shortcuts-settings')`)
  const advertised = await run(`Array.from(document.querySelectorAll('.keys__kbd'), el => el.textContent)`)
  await press('Escape')

  // A swallowed keydown must still work; a delivered one must not toggle twice.
  await run(`window.dispatchEvent(new KeyboardEvent('keyup', {key:'E',code:'KeyE',ctrlKey:true,shiftKey:true,bubbles:true}))`)
  await until(`document.querySelector('.panel__title')?.textContent === '导出'`)
  await press('Escape')
  await press('Ctrl+Shift+E')
  await until(`document.querySelector('.panel__title')?.textContent === '导出'`)
  await press('Escape')

  // File operations, and export with focus in each panel/control.
  await edit('# 快捷键检查\n\n保留实时未保存内容。')
  const file = path.join(OUT, 'documents/快捷键.md')
  saves.push(file)
  await press('Ctrl+S')
  await until(`${ctx}.docPath === ${JSON.stringify(file)}`)
  assert.match(await fs.readFile(file, 'utf8'), /快捷键检查/)
  for (const context of ['editor', 'settings', 'settings-input', 'shortcuts', 'history', 'palette', 'context-menu']) {
    await run('window.__mdview.closePanel()')
    await wait(80)
    await run(`${ctx}.focus()`)
    if (context.startsWith('settings')) {
      await press('Ctrl+,')
      if (context === 'settings-input') await run(`document.querySelector('.settings input').focus()`)
      const before = await text()
      await press('Ctrl+B')
      await press('Ctrl+Z')
      assert.equal(await text(), before, 'panel shortcuts must not edit the document underneath')
    } else if (context === 'shortcuts') await press('F1')
    else if (context === 'history') {
      await press('Ctrl+Alt+Y')
      await run(`document.querySelector('.history-panel button')?.focus()`)
    } else if (context === 'palette') await press('Ctrl+P')
    else if (context === 'context-menu') await run(`document.querySelector('.cm-content').dispatchEvent(new MouseEvent('contextmenu', {bubbles:true,clientX:500,clientY:180}))`)
    await press('Ctrl+Shift+E')
    assert.equal(await run(`document.querySelector('.panel__title')?.textContent`), '导出', context)
    await press('Escape')
    assert.equal(await run(`document.activeElement.classList.contains('cm-content')`), true, 'return focus after panel dismissal')
  }
  await edit('# 导出测试\n\n实时缓冲区内容，不需要先自动保存。')
  await press('Ctrl+Shift+E')
  const exported = path.join(OUT, 'documents/exported.md')
  saves.push(exported)
  await until(`!document.querySelector('.panel[aria-label="导出"] .panel__foot button').disabled`)
  await run(`document.querySelector('.panel[aria-label="导出"] .panel__foot button').click()`)
  await until(`document.querySelector('.panel[aria-label="导出"] .panel__foot').textContent.includes('已导出')`)
  assert.match(await fs.readFile(exported, 'utf8'), /实时缓冲区/)
  await shot('01-export-from-panels')
  await press('Escape')
  const sourceWindow = win
  await press('Ctrl+N')
  win = BrowserWindow.getAllWindows().find(window => window !== sourceWindow)
  assert.ok(win)
  win.setTitle('MDWisp — Shortcut verification')
  win.setSize(1440, 900)
  await until(`!!document.querySelector('.cm-content')`)
  assert.match(await sourceWindow.webContents.executeJavaScript('window.__mdview.editorContext().source'), /实时缓冲区/)
  sourceWindow.destroy()
  assert.equal(await text(), '')
  opens.push(file)
  const blankWindow = win
  await press('Ctrl+O')
  win = BrowserWindow.getAllWindows().find(window => window !== blankWindow)
  assert.ok(win)
  win.setTitle('MDWisp — Shortcut verification')
  win.setSize(1440, 900)
  await until(`!!document.querySelector('.cm-content')`)
  blankWindow.destroy()
  assert.equal(await text(), await fs.readFile(file, 'utf8'))
  opens.push(path.join(OUT, 'documents'))
  await press('Ctrl+Shift+O')
  await until(`!!document.querySelector('.sidebar')`)
  console.log('PASS: file shortcuts and export from every panel')

  // Formatting, headings, list conversions, insertion and undo aliases.
  for (const [key, expected] of [
    ['Ctrl+B', '**word**'], ['Ctrl+I', '*word*'], ['Ctrl+U', '<u>word</u>'],
    ['Ctrl+Alt+S', '~~word~~'], ['Ctrl+E', '`word`'], ['Ctrl+Shift+H', '==word==']
  ]) {
    await edit('word', 0, 4)
    await press(key)
    assert.equal(await text(), expected, key)
  }
  for (const n of [1, 2, 3, 4, 5, 6, 0]) {
    await edit('标题', 0)
    await press(`Ctrl+Shift+Digit${n}`)
    assert.equal(await text(), (n ? '#'.repeat(n) + ' ' : '') + '标题')
  }
  for (const [key, expected] of [
    ['Ctrl+Shift+Digit8', '> word'], ['Ctrl+Shift+Digit7', '- word'],
    ['Ctrl+Shift+Digit9', '1. word'], ['Ctrl+Shift+U', '- [ ] word']
  ]) {
    await edit('word', 0)
    await press(key)
    assert.equal(await text(), expected, key)
  }
  for (const [key, expected] of [
    ['Ctrl+K', '[word](url)'], ['Ctrl+Shift+C', '```'], ['Ctrl+Alt+T', '|'],
    ['Ctrl+Shift+W', '[^1]'], ['Ctrl+Alt+M', '$$'], ['Ctrl+Alt+K', '```mermaid'],
    ['Ctrl+Alt+O', '[TOC]'], ['Ctrl+Alt+H', '---']
  ]) {
    await edit('word', 0, 4)
    await press(key)
    assert.ok((await text()).includes(expected), key)
  }
  await edit('undo', 0, 4)
  await press('Ctrl+B')
  await press('Ctrl+Z')
  assert.equal(await text(), 'undo')
  await press('Ctrl+Shift+Z')
  assert.equal(await text(), '**undo**')
  await press('Ctrl+Z')
  await press('Ctrl+Y')
  assert.equal(await text(), '**undo**')
  await press('Ctrl+Shift+Alt+C')
  assert.equal(clipboard.readText(), 'undo')
  await edit('找我，再找我', 0, 2)
  await press('Ctrl+Shift+F')
  assert.deepEqual(await run(`${ctx}.selection`), { start: 4, end: 6 })
  await press('Ctrl+F')
  assert.equal(await run(`document.activeElement.name`), 'search')
  await press('F3')
  assert.deepEqual(await run(`${ctx}.selection`), { start: 0, end: 2 })
  await press('Shift+F3')
  assert.deepEqual(await run(`${ctx}.selection`), { start: 4, end: 6 })
  await press('Ctrl+H')
  assert.equal(await run(`document.activeElement.name`), 'replace')
  await press('Escape')
  await edit('word   \n\n\n\nnext\n')
  await press('Ctrl+Alt+F')
  assert.ok(!(await text()).includes('word   '))
  await edit('```js\nconst a = 1\n```\n', 10)
  await press('Ctrl+Shift+Alt+E')
  assert.match(clipboard.readText(), /const a = 1/)
  await press('Ctrl+Shift+A')
  assert.equal(await run(`${ctx}.source.slice(${ctx}.selection.start,${ctx}.selection.end)`), 'const a = 1\n')
  console.log('PASS: formatting, headings, lists, insertion, find, undo and code shortcuts')

  // Every bound table operation uses the same engine from native events.
  const table = '| 名称 | 数量 |\n| --- | --- |\n| 梨 | 2 |\n| 苹果 | 3 |\n| 桃 | 1 |\n'
  const tableKeys = ['Ctrl+Enter', 'Ctrl+Alt+N', 'Ctrl+Shift+Backspace', 'Alt+ArrowUp', 'Alt+ArrowDown',
    'Ctrl+Shift+X', 'Ctrl+Shift+Enter', 'Ctrl+Alt+Enter', 'Ctrl+Alt+Backspace', 'Alt+ArrowLeft', 'Alt+ArrowRight',
    'Ctrl+Alt+L', 'Ctrl+Alt+C', 'Ctrl+Alt+R', 'Ctrl+M', 'Ctrl+Delete', 'Alt+Enter',
    'Ctrl+Alt+ArrowUp', 'Ctrl+Alt+ArrowDown', 'Ctrl+Alt+P', 'Ctrl+Alt+X']
  for (const key of tableKeys) {
    const at = table.indexOf(key === 'Alt+ArrowLeft' || key.startsWith('Ctrl+Alt+Arrow') ? '2' : '苹果')
    await edit(table, at)
    await press(key)
    assert.notEqual(await text(), table, key + ' ' + JSON.stringify(await run(`({event:window.__lastKey,table:${ctx}.inTable,cursor:${ctx}.cursor})`)))
    if (key === 'Ctrl+M') {
      const merged = await text()
      await run(`${ctx}.select(${table.indexOf('苹果')}, ${table.indexOf('苹果')})`)
      await wait(80)
      await press('Ctrl+Shift+M')
      assert.notEqual(await text(), merged, 'split shortcut changes the merged cell')
      assert.ok(!(await text()).includes('colspan'), 'split removes the merged-cell marker')
    }
  }
  await edit(table, table.indexOf('苹果'))
  await press('Ctrl+Shift+D')
  assert.match(clipboard.readText(), /苹果/)
  await press('Ctrl+Shift+Space')
  assert.equal(await run(`${ctx}.selection.end - ${ctx}.selection.start`), table.trimEnd().length)
  await edit(table, table.indexOf('苹果'))
  clipboard.writeText('甲\t乙\n10\t20')
  await press('Ctrl+Alt+V')
  assert.match(await text(), /甲/)
  assert.match(await text(), /20/)
  await edit(table, table.indexOf('苹果'))
  await press('Delete')
  assert.match(await text(), /苹|果/, 'plain Delete must remain ordinary text editing')
  await shot('02-table-shortcuts')

  await press('Ctrl+Shift+T')
  assert.equal(await run(`document.documentElement.dataset.theme`), 'light')
  await press('Ctrl+Backslash')
  assert.equal(await run(`!!document.querySelector('.sidebar')`), false)
  await press('Ctrl+Backslash')
  await run(`window.__mdview.setSettings({theme:'dark',sidebarVisible:true})`)
  await press('Ctrl+Slash')
  await until(`!!document.querySelector('.cm-content.source-mode')`)
  await press('Ctrl+Slash')
  await until(`!document.querySelector('.cm-content.source-mode')`)
  const readingSize = () => run(`getComputedStyle(document.documentElement).getPropertyValue('--reading-size').trim()`)
  await press('Ctrl+Equal')
  assert.equal(await readingSize(), '18px')
  await press('Ctrl+Minus')
  await press('Ctrl+Minus')
  assert.equal(await readingSize(), '16px')
  await press('Ctrl+Digit0')
  assert.equal(await readingSize(), '17px')
  // Reload shows the file on disk; an unsaved draft has none and stays as typed.
  const reloadPath = await run(`${ctx}.docPath`)
  const beforeReload = await text()
  await press('Ctrl+Shift+R')
  await wait(200)
  assert.equal(await text(), reloadPath ? await fs.readFile(reloadPath, 'utf8') : beforeReload)
  // Close history dock, which remains independent of modal panels.
  if (await run(`!!document.querySelector('.history-panel')`)) await press('Ctrl+Alt+Y')

  // Large image: proportional shrinking/growing with viewport and dragged margins.
  const imageFile = path.join(OUT, 'documents/wide.svg')
  await fs.writeFile(imageFile, '<svg xmlns="http://www.w3.org/2000/svg" width="2400" height="800"><rect width="2400" height="800" fill="#496c82"/><text x="160" y="420" font-size="110" fill="white">Responsive image · 3:1</text></svg>')
  await edit('# 自适应写作\n\n正文随窗口宽度伸缩。拖动左右细线调整边距，双击恢复。\n\n', 0)
  const end = (await text()).length
  await run(`${ctx}.select(${end},${end})`)
  opens.push(imageFile)
  await press('Ctrl+Shift+I')
  assert.match(await text(), /!\[/)
  assert.ok((await text()).startsWith('# 自适应写作'), 'insertion must use the latest native cursor')
  await run(`${ctx}.select(0,0)`)
  await until(`!!document.querySelector('.live-rendered img')?.complete`)
  await wait(400)
  const metrics = () => run(`(() => {
    const content = document.querySelector('.cm-content'), s = getComputedStyle(content)
    const img = document.querySelector('.live-rendered img').getBoundingClientRect()
    return { text:content.clientWidth-parseFloat(s.paddingLeft)-parseFloat(s.paddingRight), image:img.width, ratio:img.width/img.height,
      overflow:document.querySelector('.cm-scroller').scrollWidth-document.querySelector('.cm-scroller').clientWidth }
  })()`)
  const normal = await metrics()
  assert.ok(normal.text > 900, JSON.stringify(normal))
  assert.ok(normal.image <= normal.text + 1 && normal.image > normal.text - 4, JSON.stringify(normal))
  assert.ok(Math.abs(normal.ratio - 3) < 0.02)
  await shot('03-fluid-normal')
  win.maximize()
  await wait(400)
  const maximized = await metrics()
  assert.ok(maximized.text > normal.text, JSON.stringify({ normal, maximized }))
  assert.ok(maximized.image > normal.image)
  await shot('04-fluid-maximized')
  win.unmaximize()
  await wait(250)
  win.setSize(1440, 900)
  await wait(250)

  const drag = async (side, distance) => {
    const point = await run(`(() => { const r=document.querySelector('.page-margin--${side}').getBoundingClientRect(); return {x:Math.round(r.x+r.width/2),y:Math.round(r.y+70)} })()`)
    const mouse = (type, x) => win.webContents.sendInputEvent({ type, x, y:point.y, button:'left', clickCount:1, modifiers:type==='mouseUp'?[]:['leftButtonDown'] })
    mouse('mouseDown', point.x)
    for (let n=1;n<=6;n++) { mouse('mouseMove', Math.round(point.x+distance*n/6)); await wait(20) }
    mouse('mouseUp', point.x+distance)
    await wait(250)
  }
  await drag('left', 120)
  await drag('right', -95)
  const narrow = await metrics()
  assert.ok(narrow.text < normal.text - 190, JSON.stringify({ normal, narrow }))
  assert.ok(narrow.image < normal.image - 190)
  assert.ok(Math.abs(narrow.ratio - 3) < 0.02)
  await shot('05-dragged-margins')
  const stored = JSON.parse(await fs.readFile(path.join(OUT, 'profile/settings.json'), 'utf8'))
  assert.ok(stored.pageMarginLeft > 12 && stored.pageMarginRight > 10)
  const point = await run(`(() => {
    const el = [...document.querySelectorAll('.cm-line')].find(el => el.textContent.includes('正文随窗口'))
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT)
    let node
    while (node = walker.nextNode()) {
      const index = node.textContent.indexOf('窗口')
      if (index < 0) continue
      const range = document.createRange(); range.setStart(node,index); range.setEnd(node,index+1)
      const box = range.getBoundingClientRect()
      return {x:Math.round(box.left+1),y:Math.round((box.top+box.bottom)/2)}
    }
  })()`)
  win.webContents.sendInputEvent({type:'mouseDown',...point,button:'left',clickCount:1})
  win.webContents.sendInputEvent({type:'mouseUp',...point,button:'left',clickCount:1})
  await wait(100)
  assert.equal(await run(`${ctx}.cursor`), (await text()).indexOf('窗口'), 'mouse position after margin resize')
  await press('Ctrl+S')
  win.webContents.reload()
  await wait(800)
  await until(`!!document.querySelector('.live-rendered img')?.complete`)
  assert.ok(Math.abs((await metrics()).text - narrow.text) < 3, 'margins persist on reload')
  await run(`document.querySelector('.page-margin--left').focus()`)
  await press('ArrowRight')
  assert.ok((await metrics()).text < narrow.text - 5, 'separator keyboard adjustment')
  await run(`document.querySelector('.page-margin--left').dispatchEvent(new MouseEvent('dblclick',{bubbles:true}))`)
  await wait(100)
  assert.equal(await run(`document.documentElement.style.getPropertyValue('--page-margin-left')`), '5%')
  await run(`document.querySelector('.live-rendered img').focus()`)
  await press('Z')
  await until(`document.fullscreenElement?.tagName === 'IMG'`)
  await press('Escape')
  await until('!document.fullscreenElement')
  await wait(200)
  win.setSize(900, 760)
  await wait(300)
  const small = await metrics()
  assert.ok(small.image <= small.text + 1 && small.overflow < 3, JSON.stringify(small))
  await run(`window.__mdview.setSettings({theme:'light',sidebarVisible:false})`)
  await shot('06-narrow-light')
  const missing = advertised.map(canonical).filter(binding => !used.has(binding))
  assert.deepEqual(missing, [], 'every advertised shortcut must have a native event check')
  assert.equal(saves.length + opens.length, 0)
  console.log(`PASS: ${new Set(advertised.map(canonical)).size} advertised shortcuts, export in 7 focus contexts and actual output, adaptive text/images, independent margin drag and persistence`)
}
main().then(() => { clipboard.write(originalClipboard); app.exit(0) }).catch(error => { clipboard.write(originalClipboard); console.error(error); app.exit(1) })
