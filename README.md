# s2akit

本地部署的 [sub2api](https://github.com/Wei-Shaw/sub2api) 账号测速与切换工具：主窗口管理账号，
托盘右键菜单快速测速/切换。

针对「codex 绑定 1 个 API Key → Key 绑定分组 → 分组内多个同类账号」的使用场景：分组内账号不稳定时，
用它对每个账号发起一次真实对话测试（测量首 token 时间与总耗时），选出稳定可用的账号一键启用（切换）。

## 功能

- **测速**：调用 sub2api 管理 API `POST /api/v1/admin/accounts/:id/test`（SSE 流式），客户端计时：
  请求发出 → 首个 `content` 事件 = 首 token；→ `test_complete` = 总耗时。
  未显式指定模型时的选择规则（候选均须在该账号模型列表中）：该账号上次测试的模型（持久化记忆）
  → 平台默认（OpenAI 走 `astra`，Anthropic/Antigravity 优先 `opus-5`、次选 `opus`）
  → 列表第一个非媒体文本模型。失败原因（上游 401/402/429/502 等）直接展示。
- **切换**：单个账号的启用/禁用（`schedulable`），入口有三处：主窗口表格调度开关、
  表格行右键菜单、托盘子菜单。
- **主窗口账号页**：顶栏分组胶囊切换（会记忆）；表格支持列排序与分页，行首状态竖条 +
  限流/错误徽标，首 token 按耗时着色；行右键菜单可选择模型测试、启用/禁用；
  右上角悬停登录邮箱可看当前 API Key 当天用量。无边框自绘标题栏（空白拖拽、最小化/
  最大化、关闭隐藏到托盘）。
- **主窗口设置页**：服务地址与管理员账号（含 TOTP 二步验证登录）、测速参数
  （Prompt/超时时间；批量并发数暂未开放）、托盘菜单（不透明度、当天额度刷新间隔）、
  关于（当前版本、检查更新，更新按钮在底部与保存同行）。
- **自动更新**：启动后静默检查 GitHub Releases，有新版弹窗询问，后台下载、校验通过后确认安装重启。
- **托盘常驻**：关窗即隐藏到托盘（非退出）；左键托盘图标显示/隐藏主窗口；右键菜单：
  - 顶部单行两区：左侧悬停/点击级联**分组列表**（独立子窗口，主菜单高度不变），
    右侧显示**当前使用的 API Key**（最近有流量的 key）当天用量——费用与 token 总量（M），
    悬停看请求次数与输入/输出/缓存明细（`/admin/usage/stats?period=today`，按服务器时区取自然日），
    点击强制刷新一次即时值（转圈保底 1 秒 + 到值闪光反馈）；两区独立响应事件，互不影响
  - 账号行悬停（100ms 意图延迟）或点击，在主菜单旁另起**独立子菜单窗口**（主菜单宽度不变，
    右侧放不下自动翻到左侧）：测试该账号 / 选择模型测试…（模型名过长悬停看全名）/ 启用禁用
  - 刷新账号列表 / 显示主窗口 / 退出
  - 菜单为 Acrylic 毛玻璃 + DWM 系统圆角，子菜单窗口不抢焦点，失焦/再右键/ESC 收起
- **登录**：管理员邮箱+密码（支持 TOTP 二步验证），token 自动刷新；首次调用管理接口若触发
  合规确认（423 ADMIN_COMPLIANCE_ACK_REQUIRED）会弹出确认对话框。
- **配置持久化**：`%APPDATA%/com.niq.s2akit/config.json`（含凭据与上次使用模型记忆，本机单人使用场景）。

## 开发

```bash
npm install
npm run tauri dev    # 开发调试
npm run tauri build  # 打包安装程序（输出 src-tauri/target/release/bundle/）
```

技术栈：Tauri 2 + Vue 3 + TypeScript + Vuetify 4；Rust 侧 reqwest（SSE 流式）+
opener/notification/updater/process 插件。目前主要面向 Windows（托盘/DWM/NSIS 更新包）。

## 使用提示

- 首次启动在「设置」页填 sub2api 地址（默认 `http://127.0.0.1:8080`）与管理员账号，保存并登录。
- 「账号」页选择目标分组（会记忆），按首 token 排序挑稳定账号，点调度开关或行右键菜单完成切换。
- 想看额度是否实时：点托盘菜单顶部的用量区强制刷新一次。
- 有新版本时主窗口会弹窗提示，可后台下载、安装时重启生效。
- 调试 WebView（可选）：
  `WEBVIEW2_USER_DATA_FOLDER=<空目录> WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223 npm run tauri dev`，
  再用 `node scripts/cdp-eval.mjs <js文件> [目标URL关键字]` 在页面上下文执行 JS。
  独立的 `WEBVIEW2_USER_DATA_FOLDER` 必需：与常驻实例共享用户数据目录时 WebView2 浏览器进程已存在，
  调试端口参数会被忽略。

## 注意

- sub2api 测速接口不返回耗时指标，首 token / 总耗时均由本工具在客户端测量。
- 托盘菜单页面带存活检测：弹出时若页面无响应（如 dev 服务器重启后停在错误页），会先自动重载再显示。
- 凭据明文保存在本机配置文件中，请自行评估风险。
