# 项目约定

## 分层与依赖方向

```
renderer  →  preload  →  main
      ↘        ↓        ↙
          shared/
```

- `src/shared/` 不依赖任何进程，只放类型与常量。加字段时先想清楚它是主进程私有还是跨进程契约。
- `src/main/` 不得 import 任何渲染进程代码。
- 渲染进程只能通过 `window.mdview` 访问系统能力。**不允许**在渲染进程里直接 `require('fs')`，也不允许绕过 preload 暴露的通道另开一条路。

## 路径：唯一入口

任何写进 markdown 的资源路径都必须经过 `src/main/services/paths.ts` 的 `buildRelativePath`。它会拒绝绝对路径、盘符和 `file://`。

新增涉及路径的功能时：

1. 不要自己拼字符串。找到 `buildRelativePath` 或 `toLink`。
2. 如果现有函数覆盖不了你的场景，改那个文件，不要在旁边另写一个。
3. 写完之后问自己：这个文档文件夹拷到另一台机器上，链接还成立吗？不成立就是 bug。

## 文档文件夹：一种格式

标准形态只有一种：`名称/名称.md` + `名称/名称_img/`。`DocumentMeta.inFolder` 为假的是散装文件，由 `src/main/services/organize.ts` 转换。

- 不要再引入第二种布局或布局设置。应用、转换、插件三处都只认这一种。
- 需要遍历文档里的图片引用时用 `organize.ts` 的 `mapImageLinks`，它跳过围栏代码块和行内代码。自己写正则会把示例当成引用。
- 任何按"文档所在文件夹"整体操作的功能（如 ZIP）必须先确认 `inFolder`，否则对散装文件就是在操作别人的目录。

## AI 插件

`plugin/` 是 Claude Code 插件，也是格式规范的**唯一文本来源**：`src/shared/skills/index.ts` 用 `?raw` 读 `SKILL.md`，帮助面板展示的就是插件里的内容。

- 改规则只改 `plugin/mdwisp/skills/*/SKILL.md`，不要在应用里另存一份。
- `skills/doc/scripts/check.mjs` 必须零依赖、可独立运行，不能 import 应用代码。它与 `mapImageLinks` 的扫描规则是有意重复的，改一处时两处一起改，`plugin.test.ts` 覆盖脚本行为。
- 规则与应用行为必须一致：应用写出的链接要能通过 `check.mjs`（欢迎文档在测试里被它校验）。

## 交互层：动作注册表

快捷键、右键菜单、命令面板三处界面**全部**从 `src/renderer/src/actions/registry.ts` 生成。

不允许：

- 在组件里写 `if (e.key === 'x')` 直接调业务逻辑 —— 变成一个 `ActionDef`。
- 手写菜单项的标题与快捷键提示 —— 从注册表读，否则一定会和快捷键脱节。
- 给某个面板单独维护一份能做的事的列表。

新增动作时，`group` 决定它进右键菜单的哪一段，`scope` 决定它在什么上下文可用，`key` 决定快捷键。三者都填对，三处界面自动同步。

## 状态

`src/renderer/src/state/` 下是极简的发布订阅，不是 Redux。每个 store 是一个模块级变量加一组监听器。

- 需要跨组件共享 → 加一个 store。
- 只是父子组件传递 → 用 props，不要为了让某处能读到就把它提升成 store。

## 编辑器

`src/renderer/src/editor/Editor.tsx` 是 CodeMirror 6 单页实时编辑：源码始终是 markdown 原文，`live-preview.ts` 只加装饰（隐藏标记、行样式、表格与图片 widget），不改写文本。新增语法的显示效果时加装饰，不要动文档内容；大纲（`outline/headings.ts`）与编辑器共用同一个解析器，两处对"什么算标题"的判断必须一致。

## 表格

`src/renderer/src/table/model.ts` 与 `ops.ts` 是纯函数，不碰 DOM、不依赖 React。新增表格能力时：

- 逻辑放引擎里，加测试；UI 只负责调用。
- 引擎的测试在 `table.test.ts`，改动引擎必须同步改测试。
- `parseTable` 返回的矩阵是**表头 + 数据行**，不含分隔行；分隔行由 `aligns` 在序列化时重新生成。

## 样式

- 所有颜色、间距、圆角、时长、缓动都走 `src/renderer/src/styles/tokens.css` 的 CSS 变量。**不允许**在组件样式里写 hex 色值。
- 动效只动 `transform` 与 `opacity`。需要动尺寸的用 FLIP。
- 时长必须取自 token 体系，不临时拍脑袋定一个 `320ms`。
- 深色是主主题。加浅色适配时，先去 `tokens.css` 的 `[data-theme='light']` 段补变量，不要在组件里写主题分支。

## 语言检测

`src/shared/markdown/detect.ts` 的规则表是数据驱动的。加规则时：

- **强信号必须是该语言独有的**。`function name(` 不是任何语言的强信号——每个 C 系语言都有，它会把差距压垮，让判定退化成"未确定"。
- 拿不准宁可返回 `null`。错误的高亮比没有高亮更糟。
- 改完跑 `detect.test.ts`，它同时验证"该认出来的"和"不该猜的"。

## 代码风格

- 2 空格缩进，无分号，单引号，具名导出。
- 不给不可能出现的输入写防御性检查。项目有导表校验和单元测试；要的是领域校验，不是 `typeof x === 'number'`。
- 注释只写"为什么"，不写"做了什么"。读代码就知道的事不要注释。
- 修改已有文件用最小 diff，不要顺手重构无关代码。

## 测试与验证

```bash
pnpm test             # 单元测试
pnpm typecheck        # 两个 tsconfig 都要过
claude plugin validate plugin   # 改过 plugin/ 之后
```

改完 UI 后的视觉验证：

```bash
pnpm exec electron-vite build
pnpm exec electron scripts/shoot.cjs design-review/round-N
```

截图按轮次存档，用于对比是否退化。

## Git

- `main` 不接受直接提交。
- 功能分支命名：`feat_SC_<简述>` / `fix_C_<简述>`（S 后端、C 前端、SC 前后端）。
- 提交信息用英文，正文说明**为什么**这样改，不是罗列改了哪些文件。
