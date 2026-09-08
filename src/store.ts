import { reactive } from "vue";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AccountBrief,
  AppConfig,
  AppErrorPayload,
  AuthInfo,
  ComplianceInfo,
  GroupBrief,
  LoginReply,
  ModelBrief,
  TestAllSummary,
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
  testingAll: false,
  loadingAccounts: false,
  loadingGroups: false,
  saving: false,
  loggingIn: false,
  /** 登录返回 requires_2fa 时的待验证状态 */
  pending2fa: null as { tempToken: string } | null,
  /** 非 null 时显示合规确认对话框 */
  compliance: null as { info: ComplianceInfo | null } | null,
  snack: { show: false, message: "", color: "info" as string },
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
  store.config = await invoke<AppConfig>("get_config");
  await listen<AccountBrief[]>("accounts-updated", (ev) => {
    store.accounts = ev.payload;
  });
  await listen<TestProgress>("test-progress", (ev) => applyProgress(ev.payload));
  await listen<TestAllSummary>("test-all-done", () => {
    store.testingAll = false;
  });
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

export async function testAll() {
  store.testingAll = true;
  const ch = new Channel<TestProgress>();
  ch.onmessage = applyProgress;
  try {
    await invoke<TestAllSummary>("test_all", { onEvent: ch });
  } catch (e) {
    store.testingAll = false;
    handleErr(e);
  }
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
    store.testingAll = false;
    snack("已退出登录", "info");
  } catch (e) {
    handleErr(e);
  }
}

async function afterLogin() {
  await refreshGroups();
  await refreshAccounts();
}
