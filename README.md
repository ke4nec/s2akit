# s2akit

本地部署的 [sub2api](https://github.com/Wei-Shaw/sub2api) 账号测速与切换托盘小工具。

针对「codex 绑定 1 个 API Key → Key 绑定分组 → 分组内多个同类账号」的使用场景：分组内账号不稳定时，
用它对每个账号发起一次真实对话测试（测量首 token 时间与总耗时），选出稳定可用的账号一键启用（切换）。

## 功能

- **测速**：调用 sub2api 管理 API `POST /api/v1/admin/accounts/:id/test`（SSE 流式），客户端计时：
  请求发出 → 首个 `content` 事件 = 首 token；→ `test_complete` = 总耗时。
  支持单账号测试与分组批量测试（并发数可配，默认 2），失败原因（上游 401/402/429/502 等）直接展示。
- **切换**：`POST /:id/schedulable` 启用目标账号（按你的选择，仅启用目标、不禁用其他）。
- **托盘常驻**：关窗即隐藏到托盘（非退出）；左键托盘图标显示/隐藏主窗口；右键菜单：
  - 顶部单行两区：左侧点击切换分组（选模型时显示账号名、点击返回），右侧显示**当前使用的 API Key**
    （最近有流量的 key）当天用量——费用与 token 总量（M），悬停可看请求次数与输入/输出/缓存明细
    （`/admin/usage/stats?period=today`，按服务器时区取自然日）；两区独立响应事件，互不影响
  - 分组内账号列表（●=当前启用，⚠=错误状态），**点击账号即切换（启用）**；行尾显示最近测试结果
    （首 token 毫秒数按耗时着色，失败显示 ✗；主窗口测过的同样显示，结果由后端缓存跨窗口共享）
  - 账号行右键：选择模型测试… / 禁用该账号
  - 测试全部账号（后台执行，完成后系统通知最快账号与失败数）
  - 刷新账号列表 / 显示主窗口 / 退出
- **登录**：管理员邮箱+密码（支持 TOTP 二步验证），token 自动刷新；首次调用管理接口若触发
  合规确认（423 ADMIN_COMPLIANCE_ACK_REQUIRED）会弹出确认对话框。
- **配置持久化**：`%APPDATA%/com.niq.s2akit/config.json`（含凭据，本机单人使用场景）。

## 开发

```bash
npm install
npm run tauri dev    # 开发调试
npm run tauri build  # 打包安装程序（输出 src-tauri/target/release/bundle/）
```

技术栈：Tauri 2 + Vue 3 + TypeScript + Vuetify 3；Rust 侧 reqwest（SSE 流式）+ 托盘/通知插件。

## 使用提示

- 首次启动在「设置」页填 sub2api 地址（默认 `http://127.0.0.1:8080`）与管理员账号，保存并登录。
- 「账号」页选择目标分组（会记忆），查看账号状态与测试结果，点行内「启用」完成切换。
- 调试 WebView（可选）：
  `WEBVIEW2_USER_DATA_FOLDER=<空目录> WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223 npm run tauri dev`，
  再用 `node scripts/cdp-eval.mjs <js文件> [目标URL关键字]` 在页面上下文执行 JS。
  独立的 `WEBVIEW2_USER_DATA_FOLDER` 必需：与常驻实例共享用户数据目录时 WebView2 浏览器进程已存在，
  调试端口参数会被忽略。

## 注意

- sub2api 测速接口不返回耗时指标，首 token / 总耗时均由本工具在客户端测量。
- 托盘菜单页面带存活检测：弹出时若页面无响应（如 dev 服务器重启后停在错误页），会先自动重载再显示。
- 凭据明文保存在本机配置文件中，请自行评估风险。
