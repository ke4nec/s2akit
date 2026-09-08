# s2akit · Agent 规则

## Git 提交信息（Conventional Commits）

格式：

```text
<type>(<scope>): <一句话中文概述>

- 中文 bullet 补充细节（可选，有实质改动时写）
```

- `type` 必填其一：`feat`（新功能）、`fix`（修复 bug）、`docs`（仅文档）、
  `style`（不影响逻辑的格式/样式）、`refactor`（重构）、`perf`（性能优化）、
  `test`（测试）、`chore`（构建/依赖/杂项）、`revert`（回滚）。
- `scope` 可选，为模块名：`tray`、`toast`、`ui`、`accounts`、`settings`、
  `config`、`build` 等；一次改动横跨多模块时省略 scope。
- 标题 ≤ 72 字符，用中文；正文 bullet 只写“做了什么”，不写实现过程。
- 一次提交只做一类事；混了多个 type 时按主体取 type，次要改动在正文注明。
- 提交前确认：`cargo test`（Rust）、`npx vue-tsc --noEmit`（前端）通过；
  不提交密钥、本地路径等敏感信息。

示例：

```text
fix(tray): 托盘菜单底边贴住光标，允许盖住任务栏

- menu_origin 改用显示器完整矩形定位（原生 TPM_BOTTOMALIGN 语义）
- 卡片级右键点消，保留“再右键收起”体验
```
