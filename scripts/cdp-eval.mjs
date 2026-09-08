// 通过 CDP 在 s2akit webview 页面上下文执行 JS。
// 前置：带调试端口启动应用，例如（独立用户数据目录，避免与常驻实例共享 WebView2 进程）：
//   WEBVIEW2_USER_DATA_FOLDER=<某空目录> WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223 \
//     npm run tauri dev
// 用法：node scripts/cdp-eval.mjs <js文件> [目标URL关键字，默认 tray-menu]
import { readFileSync } from "node:fs";

const file = process.argv[2];
const keyword = process.argv[3] || "tray-menu";
if (!file) {
  console.error("用法: node scripts/cdp-eval.mjs <js文件> [目标URL关键字]");
  process.exit(1);
}

const targets = await (await fetch("http://127.0.0.1:9223/json")).json();
const page = targets.find(
  (t) => t.type === "page" && t.url.includes(keyword) && t.webSocketDebuggerUrl,
);
if (!page) {
  console.error("targets:", targets.map((t) => `${t.type} ${t.url}`).join("\n"));
  throw new Error(`no target matching ${keyword}`);
}
console.error(`target: ${page.url}`);

const expr = readFileSync(file, "utf8");

const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const mid = ++id;
    pending.set(mid, { resolve, reject });
    ws.send(JSON.stringify({ id: mid, method, params }));
  });
}
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
  }
};
await new Promise((r, j) => {
  ws.onopen = r;
  ws.onerror = j;
});

// 注意：不要开 replMode，它会与 awaitPromise 冲突（Promise 结果序列化为空对象）
const out = await send("Runtime.evaluate", {
  expression: expr,
  awaitPromise: true,
  returnByValue: true,
});
console.log(JSON.stringify(out.result?.value ?? out, null, 2));
if (out.exceptionDetails) {
  console.error("exception:", JSON.stringify(out.exceptionDetails, null, 2));
}
// 等关闭帧发完再退进程，避免 TCP 硬断
await new Promise((r) => {
  ws.onclose = r;
  ws.close();
  setTimeout(r, 1000);
});
process.exit(0);
