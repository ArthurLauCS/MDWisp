# 当前状态

## 已完成

| 模块 | 状态 |
| :--- | :--- |
| 文档即文件夹（图片相对路径） | 完成，路径生成由 `paths.ts` 强制 |
| 图片插入（剪贴板 / 拖拽 / 文件选择） | 完成，含内容哈希去重与命名策略 |
| 导出 | 纯 Markdown、ZIP、单文件 HTML、PDF 四种全部可用；写出前预览将删除或未包含的内容 |
| 代码语言本地检测 | 完成，三层管线；悬停或编辑时显示语言选择器，可自动检测或手动指定 |
| 表格引擎 | 30 / 44 个动作接入；有快捷键的表格操作全部接通 |
| 编辑器 | CodeMirror 6 单页实时编辑；本文图片位于左侧栏底部 |
| 文档流程 | 新建、打开文件、跨文档跳转和再次启动使用独立窗口；首次保存选址；未命名无自动保存/快照；关闭确认与跨窗口保存冲突保护 |
| 侧栏与大纲 | 文件 / 大纲切换；H1–H6 层级折叠与定位；左右侧栏可拖动并记住宽度，统一面板样式 |
| 快捷键 / 右键菜单 / 命令面板 | 三者同源；71 个快捷键经实际按键回归，导出覆盖其他面板获得焦点的场景 |
| 正文布局 | 宽度自适应；正文左右边距直接拖动并记住；图片按比例适配正文宽度 |
| 会话恢复 | 每个窗口刷新时恢复自身目录和文档；冷启动恢复最后写入的会话；未命名草稿不跨退出保留 |
| 历史版本 | 右侧可收起工具栏；自动刷新、差异与全文查看、恢复前备份及撤销；连续相同内容去重 |
| 整理为文档文件夹 | 散装 md 转成 `名称/名称.md + 名称_img/`；先预览，可选保留或回收原文件、是否下载网络图片；代码里的示例不改 |
| 文件关联 | 安装时注册 `.md` / `.markdown`；每次双击均打开新窗口，后台进程共享历史写入服务 |
| Claude Code 插件 | `plugin/` 随安装包分发；`/mdwisp:doc` 按文档文件夹格式写文档（不生成图片 / 尽量生成图片），带校验脚本 |
| Codex / Cursor 技能 | 使用说明中安装到项目 `.agents/skills/`；规则与 Claude Code 同源，已有自定义内容不覆盖 |
| 代码块与右键子菜单 | 三个反引号加回车创建代码块；块下可继续写正文；子菜单独立浮层与窗口边缘避让 |
| 欢迎文档 + AI 协作规则 | 随安装包分发，从使用说明中打开；规则文本以插件的 `SKILL.md` 为唯一来源 |
| 设置面板 | 独立入口；外观与字体、文档与保存、编辑器、图片、快捷键分类；字体配置重启保留 |
| 视觉体系 | 深色优先；中文圆体、英文/代码等宽、装饰标题字体可分别设置；动效 token 化 |
| 打包 | MDWisp 0.2.0 中英 NSIS 安装包可用，`stock/`、插件、导出样式与字体随包分发；图标源文件为 `build/icon.svg` |

## 未完成

- **14 个无快捷键的表格动作** —— 五种表格样式、CSV / JSON 转换等尚未从动作注册表接入。

## 已知的取舍

- **渲染 JavaScript 约 1.95MB，内置中文字体约 22.5MB**。保留完整字体以覆盖用户文档中的汉字，字体授权随包分发。
- **`--accent` 是五个 CSS 变量**，用户自定义强调色时，其余四个由 `color-mix`
  推导。浅色模式下 hover 的明暗方向因此可能不理想 —— 一个十六进制值推导不出
  随主题翻转的规则，这个代价不值得更多代码去换。
- **整理不改写非图片链接**。散装文档里指向旁边其他文件的相对链接（如 `[另一篇](./b.md)`）在移入文件夹后会失效，需要手动处理。
- **整理后的文档是新路径，历史版本不跟随**。历史按路径归档；保留原文件时原文件的历史仍在。
- **不能自行设为默认打开方式**。Windows 只允许用户在系统设置里选择，应用只注册为候选并提供跳转。
- **插件没有做过真实对话的端到端验证**。清单经 `claude plugin validate` 通过，校验脚本有单元测试；AI 实际按规则产出的效果需要在使用中观察。
- **表格样式类操作没有存储位置**。markdown 表格语法里没有放样式的地方，
  需要额外的标记方案。
- **`sortByColumn` 靠首行是否非空来判断表头**。表头单元格本身为空时判断会失准。

## 验证方式

```bash
pnpm test             # 357 个测试
pnpm typecheck        # 主进程 + 渲染进程
pnpm dist             # 打 NSIS 安装包
pnpm exec electron scripts/make-icon.cjs   # 改过 build/icon.svg 后重新生成 build/icon.png

pnpm exec electron scripts/shoot.cjs design-review/round-N   # 视觉走查截图
pnpm exec electron scripts/workflow-check.cjs design-review/workflow-N  # 文档全流程与设置回归
pnpm exec electron scripts/shortcuts-layout-check.cjs design-review/shortcuts-layout-N  # 全部快捷键、导出与正文缩放/拖动
pnpm exec electron scripts/export-check.cjs design-review/export-check-N  # front matter 显示与四种导出的真实产物
pnpm exec electron scripts/launch-check.cjs design-review/launch-check-N  # 启动参数打开文档、第二次启动复用窗口
pnpm exec electron scripts/organize-check.cjs design-review/organize-check-N  # 散装提示、ZIP 拦截、整理全流程
pnpm exec electron scripts/code-block-menu-check.cjs design-review/code-block-menu-N  # 围栏输入、语言选择、独立子菜单
node scripts/packaged-check.cjs design-review/packaged-N  # 安装包资源、四种导出和技能安装
claude plugin validate .             # GitHub 仓库入口
claude plugin validate plugin        # 插件与市场清单
```

截图按轮次存档在 `design-review/`，用于对比是否退化。
