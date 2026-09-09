import { reactive } from "vue";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import type {
  AccountBrief,
  AppConfig,
  AppErrorPayload,
  AuthInfo,
  ComplianceInfo,
  GroupBrief,
  LoginReply,
  ModelBrief,
  TestProgress,
  TestResult,
} from "./types";

export const store = reactive({
  tab: "accounts" as "accounts" | "settings",
  config: null as AppConfig | null,
  auth: null as AuthInfo | null,
  groups: [] as GroupBrief[],
  accounts: [] as AccountBrief[],
  results: {} as Record<number, TestResult>,
  testingIds: new Set<number>(),
  loadingAccounts: false,
  loadingGroups: false,
  saving: false,
  loggingIn: false,
  /** 登录返回 requires_2fa 时的待验证状态 */
  pending2fa: null as { tempToken: string } | null,
  /** 非 null 时显示合规确认对话框 */
  compliance: null as { info: ComplianceInfo | null } | null,
  snack: { show: false, message: "", color: "info" as string },
  /** 应用更新流程：available 弹窗询问 → downloading 带进度 → ready 校验通过待安装 */
  updater: {
    current: "",
    version: "",
    notes: "",
    status: "idle" as "idle" | "available" | "downloading" | "ready",
    checking: false,
    installing: false,
    dialog: false,
    progress: 0,
    total: 0,
  },
});

export function snack(message: string, color = "info") {
  store.snack.show = false;
  requestAnimationFrame(() => {
    store.snack.message = message;
    store.snack.color = color;
    store.snack.show = true;
  });
}

function toErr(e: unknown): AppErrorPayload {
  if (typeof e === "object" && e !== null && "message" in e) {
    return e as AppErrorPayload;
  }
  return { message: String(e), code: null };
}

function isComplianceErr(e: unknown): boolean {
  return toErr(e).code === "ADMIN_COMPLIANCE_ACK_REQUIRED";
}

export function handleErr(e: unknown) {
  const err = toErr(e);
  if (err.code === "ADMIN_COMPLIANCE_ACK_REQUIRED") {
    store.compliance = { info: null };
    void loadComplianceInfo();
  } else {
    snack(err.message, "error");
  }
}

async function loadComplianceInfo() {
  try {
    const info = await invoke<ComplianceInfo>("get_compliance");
    if (store.compliance) store.compliance.info = info;
  } catch {
    // 获取失败时对话框仍可确认（Rust 侧有默认 phrase）
  }
}

export async function acceptCompliance() {
  try {
    await invoke("accept_compliance");
  } catch (e) {
    handleErr(e);
    return;
  }
  store.compliance = null;
  snack("合规确认成功，正在加载数据…", "success");
  if (store.auth) {
    await refreshGroups();
    await refreshAccounts();
  }
}

function applyProgress(p: TestProgress) {
  if (p.kind === "start" || p.kind === "content") {
    store.testingIds.add(p.account_id);
  } else if (p.kind === "result" && p.result) {
    store.results[p.account_id] = p.result;
    store.testingIds.delete(p.account_id);
  }
}

export async function init() {
  store.updater.current = await getVersion();
  store.config = await invoke<AppConfig>("get_config");
  await listen<AccountBrief[]>("accounts-updated", (ev) => {
    store.accounts = ev.payload;
  });
  await listen<TestProgress>("test-progress", (ev) => applyProgress(ev.payload));
  await listen<null>("auth-changed", async () => {
    store.auth = await invoke<AuthInfo | null>("get_auth");
  });
  // 认证状态在 listener 注册完成后再读取：
  // 后端 startup_init 可能在 WebView 加载完成前就登录并发出 auth-changed
  store.auth = await invoke<AuthInfo | null>("get_auth");
  if (store.auth) {
    await refreshGroups(false);
    await refreshAccounts(false);
  }
}

export async function refreshGroups(notify = true) {
  if (!store.auth) return;
  store.loadingGroups = true;
  try {
    store.groups = await invoke<GroupBrief[]>("list_groups");
  } catch (e) {
    // 合规确认门槛必须提示（其余错误仅在显式操作时提示）
    if (notify || isComplianceErr(e)) handleErr(e);
  } finally {
    store.loadingGroups = false;
  }
}

export async function refreshAccounts(notify = true) {
  if (!store.auth) return;
  store.loadingAccounts = true;
  try {
    store.accounts = await invoke<AccountBrief[]>("list_accounts");
  } catch (e) {
    if (notify || isComplianceErr(e)) handleErr(e);
  } finally {
    store.loadingAccounts = false;
  }
}

export async function selectGroup(groupId: number) {
  try {
    await invoke("select_group", { groupId });
    if (store.config) store.config.group_id = groupId;
    await refreshAccounts();
  } catch (e) {
    handleErr(e);
  }
}

export async function testAccount(accountId: number, model?: string | null) {
  const ch = new Channel<TestProgress>();
  ch.onmessage = applyProgress;
  try {
    await invoke("test_account", { accountId, model: model ?? null, onEvent: ch });
  } catch (e) {
    handleErr(e);
  }
}

export async function getAccountModels(accountId: number): Promise<ModelBrief[]> {
  return invoke<ModelBrief[]>("get_account_models", { accountId });
}

export async function setSchedulable(accountId: number, schedulable: boolean) {
  try {
    await invoke("set_schedulable", { accountId, schedulable });
  } catch (e) {
    handleErr(e);
  }
}

export async function saveConfig(cfg: AppConfig) {
  store.saving = true;
  try {
    await invoke("save_config", { config: cfg });
    store.config = { ...cfg };
    snack("设置已保存", "success");
  } catch (e) {
    handleErr(e);
  } finally {
    store.saving = false;
  }
}

export async function login() {
  store.loggingIn = true;
  try {
    const reply = await invoke<LoginReply>("login", { email: null, password: null });
    if (reply.requires_2fa) {
      store.pending2fa = { tempToken: reply.temp_token };
      snack("该账号开启了二步验证，请输入验证码", "info");
    } else {
      store.auth = await invoke<AuthInfo | null>("get_auth");
      snack("登录成功", "success");
      await afterLogin();
    }
  } catch (e) {
    handleErr(e);
  } finally {
    store.loggingIn = false;
  }
}

export async function submit2fa(code: string) {
  if (!store.pending2fa) return;
  try {
    await invoke("login_2fa", { tempToken: store.pending2fa.tempToken, code });
    store.pending2fa = null;
    store.auth = await invoke<AuthInfo | null>("get_auth");
    snack("登录成功", "success");
    await afterLogin();
  } catch (e) {
    handleErr(e);
  }
}

export async function logout() {
  try {
    await invoke("logout");
    store.auth = null;
    store.groups = [];
    store.accounts = [];
    store.results = {};
    store.testingIds.clear();
    snack("已退出登录", "info");
  } catch (e) {
    handleErr(e);
  }
}

async function afterLogin() {
  await refreshGroups();
  await refreshAccounts();
}

/** 待安装的更新资源（后端句柄），丢弃时需 close 释放 */
let pendingUpdate: Update | null = null;

/** 弹更新对话框；主窗口可能已隐藏到托盘，顺带带出主窗口保证提示可见 */
function showUpdateDialog() {
  store.updater.dialog = true;
  void invoke("open_main_window").catch(() => {});
}

export async function checkForUpdates(manual = false) {
  if (store.updater.status === "ready") {
    // 已有校验通过的更新待安装，直接弹窗确认即可
    showUpdateDialog();
    return;
  }
  if (store.updater.checking || store.updater.status === "downloading") return;
  store.updater.checking = true;
  try {
    const stale = pendingUpdate;
    pendingUpdate = null;
    void stale?.close();
    const update = await check({ timeout: 20000 });
    if (update) {
      pendingUpdate = update;
      store.updater.version = update.version;
      store.updater.notes = update.body ?? "";
      store.updater.status = "available";
      showUpdateDialog();
    } else if (manual) {
      snack(`已是最新版本 v${store.updater.current}`, "success");
    }
  } catch (e) {
    // 自动检查失败保持静默（离线/网络波动不打扰启动），手动检查才提示
    if (manual) handleErr(e);
  } finally {
    store.updater.checking = false;
  }
}

export async function downloadUpdate() {
  if (!pendingUpdate || store.updater.status === "downloading") return;
  store.updater.status = "downloading";
  store.updater.progress = 0;
  store.updater.total = 0;
  try {
    // download 正常返回即代表 minisign 签名校验通过（完整性检查），
    // 安装包暂存后端，等待用户确认 install
    await pendingUpdate.download((ev: DownloadEvent) => {
      if (ev.event === "Started") {
        store.updater.total = ev.data.contentLength ?? 0;
      } else if (ev.event === "Progress") {
        store.updater.progress += ev.data.chunkLength;
      }
    });
    store.updater.status = "ready";
    // 下载期间用户可能收起对话框转后台，完成后重新弹出确认安装
    showUpdateDialog();
  } catch (e) {
    store.updater.status = "available";
    handleErr(e);
  }
}

export async function installUpdate() {
  if (!pendingUpdate || store.updater.installing) return;
  store.updater.installing = true;
  try {
    // Windows：拉起 NSIS 安装器并退出当前进程，安装完成后自动重启应用；
    // relaunch 是非 Windows 平台的兜底（正常执行不到）
    await pendingUpdate.install();
    await relaunch();
  } catch (e) {
    store.updater.installing = false;
    handleErr(e);
  }
}

/** 关闭更新对话框：available 态丢弃本次更新，downloading/ready 态保留后台继续 */
export function dismissUpdate() {
  store.updater.dialog = false;
  if (store.updater.status === "available") {
    store.updater.status = "idle";
    store.updater.version = "";
    store.updater.notes = "";
    const stale = pendingUpdate;
    pendingUpdate = null;
    void stale?.close();
  }
}
