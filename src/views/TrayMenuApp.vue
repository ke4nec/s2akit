<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AccountBrief, KeyUsageToday, ModelBrief, TestResult } from "../types";

const accounts = ref<AccountBrief[]>([]);
const loading = ref(false);
const groupName = ref("");
const groups = ref<{ id: number; name: string }[]>([]);
const currentGroupId = ref<number | null>(null);
/** 菜单内容视图：账号列表 / 分组选择 / 账号操作 / 模型选择 */
const view = ref<"accounts" | "groups" | "account" | "models">("accounts");
const actionAccount = ref<AccountBrief | null>(null);
const modelAccount = ref<AccountBrief | null>(null);
const models = ref<ModelBrief[]>([]);
const loadingModels = ref(false);
const keyUsage = ref<KeyUsageToday | null>(null);
/** 最近测试结果（后端缓存，主窗口测过的也能显示） */
const results = ref<Record<string, TestResult>>({});
const testingIds = ref(new Set<number>());

/** token 数格式化为多少 M（100M 以上取整、10M 以上 1 位小数、否则 2 位） */
function fmtM(n: number): string {
  const m = n / 1e6;
  return `${m >= 100 ? m.toFixed(0) : m >= 10 ? m.toFixed(1) : m.toFixed(2)}M`;
}

function fmtCost(c: number): string {
  return `$${c >= 1000 ? c.toFixed(0) : c >= 100 ? c.toFixed(1) : c.toFixed(2)}`;
}

const usageTitle = computed(() => {
  const u = keyUsage.value;
  if (!u) return "";
  return `「${u.key_name}」今日 ${u.requests} 次请求\n输入 ${fmtM(u.input_tokens)} · 输出 ${fmtM(u.output_tokens)} · 缓存 ${fmtM(u.cache_tokens)}`;
});

function hide() {
  view.value = "accounts";
  void getCurrentWindow().hide();
}

// ---------- 窗口高度贴合内容 ----------
// 测量菜单真实内容高度回报后端（fit_menu）：窗口随之收缩并按锚点重定位，
// 菜单底角始终贴住右键点击位置。scrollHeight 是不受 max-height 约束的自然高度，
// 账号多到超出窗口时也能据此撑开窗口
let lastFitH = 0;
async function fitToContent(force = false) {
  const el = document.querySelector<HTMLElement>(".tray-menu-card");
  if (!el) return;
  const extra = el.offsetHeight - el.clientHeight; // 边框等盒模型外扩
  const h = Math.ceil(Math.max(el.scrollHeight, el.clientHeight) + extra);
  if (!force && Math.abs(h - lastFitH) < 1) return;
  lastFitH = h;
  try {
    await invoke("fit_menu", { height: h });
  } catch {
    lastFitH = 0; // 命令失败时下次重新回报
  }
}
function scheduleFit(force = false) {
  requestAnimationFrame(() => void fitToContent(force));
}

// 视图/模型列表切换会改变内容高度，联动重贴合
watch([view, models, loadingModels], () => scheduleFit());

/** 顶部左侧区点击：账号↔分组切换；账号操作/模型视图点击返回账号列表 */
function onHeaderClick() {
  if (view.value === "models" || view.value === "account") view.value = "accounts";
  else view.value = view.value === "groups" ? "accounts" : "groups";
}

async function reload() {
  loading.value = true;
  // 用量查询与账号列表并行，不阻塞主内容加载
  const usageP = invoke<KeyUsageToday | null>("get_key_usage_today").catch(() => null);
  const resultsP = invoke<Record<string, TestResult>>("get_last_results").catch(() => ({}));
  try {
    accounts.value = await invoke<AccountBrief[]>("list_accounts", { groupId: null });
    groups.value = await invoke<{ id: number; name: string }[]>("list_groups");
    const cfg = await invoke<{ group_id: number | null }>("get_config");
    currentGroupId.value = cfg.group_id;
    groupName.value = groups.value.find((g) => g.id === cfg.group_id)?.name ?? "全部账号";
    keyUsage.value = await usageP;
    results.value = await resultsP;
  } catch {
    // 后端未登录等错误：静默，主窗口会有提示
    keyUsage.value = await usageP;
  } finally {
    loading.value = false;
    scheduleFit();
  }
}

async function pickGroup(g: { id: number; name: string }) {
  view.value = "accounts";
  if (g.id === currentGroupId.value) return;
  try {
    await invoke("select_group", { groupId: g.id });
  } catch {
    hide();
    return;
  }
  await reload();
}

async function toggleAccount(a: AccountBrief) {
  try {
    await invoke("set_schedulable", {
      accountId: a.id,
      schedulable: !a.schedulable,
    });
    await reload();
  } catch {
    hide();
  }
}

async function testAll() {
  hide();
  const ch = new Channel<unknown>();
  ch.onmessage = () => {};
  try {
    await invoke("test_all", { onEvent: ch });
  } catch {
    // 主窗口会收到错误提示
  }
}

/** 单击账号行：进入该账号的二级操作菜单（不直接启停，启停也在二级菜单里） */
function openAccountMenu(a: AccountBrief) {
  actionAccount.value = a;
  view.value = "account";
}

async function testAccount(a: AccountBrief, model?: string) {
  view.value = "accounts";
  testingIds.value.add(a.id);
  try {
    const r = await invoke<TestResult>("tray_test_account", {
      accountId: a.id,
      model: model ?? null,
    });
    results.value = { ...results.value, [String(a.id)]: r };
  } catch {
    // 失败会走系统通知
  } finally {
    testingIds.value.delete(a.id);
  }
}

/** 打开某账号的模型选择列表 */
function openModels(a: AccountBrief) {
  modelAccount.value = a;
  models.value = [];
  view.value = "models";
  loadingModels.value = true;
  invoke<ModelBrief[]>("get_account_models", { accountId: a.id })
    .then((m) => {
      models.value = m;
    })
    .catch(() => {
      view.value = "accounts";
    })
    .finally(() => {
      loadingModels.value = false;
    });
}

/** 首 token 时长着色：与主窗口阈值一致 */
function ftColor(ms: number): string {
  if (ms < 2000) return "text-success";
  if (ms < 5000) return "text-warning";
  return "text-error";
}

/** 二级菜单里的启停：回账号列表并刷新，让行首圆点立即反映新状态 */
async function toggleFromMenu() {
  if (!actionAccount.value) return;
  const a = actionAccount.value;
  view.value = "accounts";
  await toggleAccount(a);
}

async function openMain() {
  hide();
  await invoke("open_main_window");
}

async function quit() {
  await invoke("quit_app");
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    if (view.value !== "accounts") view.value = "accounts";
    else hide();
  }
}

let unlisten: (() => void) | null = null;
let ro: ResizeObserver | null = null;
onMounted(async () => {
  // 在挂载后标记：script setup 顶层会在模块导入时执行，
  // 而主窗口同样会静态 import 本组件，写在顶层会污染主窗口的 documentElement
  document.documentElement.classList.add("s2a-tray-doc");
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
  unlisten = await listen("tray-menu-shown", () => {
    void invoke("menu_pong");
    // 每次弹出都从根视图开始：Rust 侧隐藏（再右键收起/失焦）不经过前端 hide()，
    // view 会残留上次的子视图，重开时应回到账号列表（原生菜单行为）
    view.value = "accounts";
    // 每次弹出后端都会先按估算高度摆放窗口，需强制按当前内容重新贴合定位
    scheduleFit(true);
    void reload();
  });
  window.addEventListener("keydown", onKey);
  // 账号列表到达、视图切换等任何盒高变化都重新贴合
  // （内容超出窗口时盒高不变，另由 reload/watch 显式触发）
  ro = new ResizeObserver(() => scheduleFit());
  const card = document.querySelector(".tray-menu-card");
  if (card) ro.observe(card);
  // 页面能执行到这里即证明加载成功，告知后端菜单存活
  void invoke("menu_pong");
  void reload();
});
onBeforeUnmount(() => {
  unlisten?.();
  ro?.disconnect();
  document.documentElement.classList.remove("s2a-tray-doc");
  window.removeEventListener("keydown", onKey);
});
</script>

<template>
  <v-app>
    <v-main class="tray-wrap">
      <!-- 卡片级右键=原生式点消：菜单底边贴住光标，会盖住托盘图标，
           在图标原地再右键会落到本窗口，用点消保留“再右键收起”的体验。
           账号行右键走二级菜单（行上 .stop，不冒泡到这里） -->
      <v-card elevation="0" rounded="0" class="tray-menu-card" @contextmenu.prevent="hide">
        <!-- 顶部一行：左侧分组/模型区可点击切换视图，右侧当前 key 当天用量仅悬停看明细 -->
        <div class="tray-header">
          <button class="header-btn" type="button" @click="onHeaderClick">
            <span class="header-title">{{
              view === "models"
                ? (modelAccount?.name ?? "选择模型")
                : view === "account"
                  ? (actionAccount?.name ?? "账号操作")
                  : groupName
            }}</span>
            <v-icon
              :icon="view === 'models' || view === 'account' ? 'mdi-chevron-left' : view === 'groups' ? 'mdi-chevron-up' : 'mdi-chevron-down'"
              size="14"
              color="primary"
            />
          </button>
          <span v-if="keyUsage" class="usage-stats" :title="usageTitle">
            <v-icon icon="mdi-chart-areaspline" size="12" color="primary" />
            今日 {{ fmtCost(keyUsage.cost) }} · {{ fmtM(keyUsage.total_tokens) }}
          </span>
        </div>
        <v-divider />

        <div class="tray-list">
          <v-progress-linear v-if="loading || (view === 'models' && loadingModels)" indeterminate color="primary" height="2" />
          <!-- 分组选择 -->
          <v-list v-if="view === 'groups'" density="compact" class="py-0 bg-transparent">
            <v-list-item v-for="g in groups" :key="g.id" density="compact" @click="pickGroup(g)" @contextmenu.stop.prevent>
              <template #prepend>
                <v-icon
                  :icon="g.id === currentGroupId ? 'mdi-check' : 'mdi-circle-medium'"
                  :color="g.id === currentGroupId ? 'primary' : 'grey'"
                  size="14"
                />
              </template>
              <v-list-item-title class="text-caption">{{ g.name }}</v-list-item-title>
            </v-list-item>
          </v-list>
          <!-- 模型选择：自动 + 该账号的模型列表 -->
          <v-list v-else-if="view === 'models'" density="compact" class="py-0 bg-transparent">
            <v-list-item density="compact" @click="modelAccount && testAccount(modelAccount)" @contextmenu.stop.prevent>
              <template #prepend>
                <v-icon icon="mdi-flash" size="14" color="primary" />
              </template>
              <v-list-item-title class="text-caption">自动选择模型</v-list-item-title>
            </v-list-item>
            <v-list-item
              v-for="m in models"
              :key="m.id"
              density="compact"
              @click="modelAccount && testAccount(modelAccount, m.id)"
              @contextmenu.stop.prevent
            >
              <template #prepend>
                <v-icon icon="mdi-chat-processing-outline" size="14" color="grey" />
              </template>
              <v-list-item-title class="text-caption">
                {{ m.display_name || m.id }}
              </v-list-item-title>
            </v-list-item>
            <div v-if="!loadingModels && !models.length" class="text-caption text-disabled pa-3">
              该账号没有可用的模型列表
            </div>
          </v-list>
          <!-- 账号操作二级菜单：单击账号行进入 -->
          <v-list v-else-if="view === 'account' && actionAccount" density="compact" class="py-0 bg-transparent">
            <v-list-item density="compact" @click="testAccount(actionAccount)" @contextmenu.stop.prevent>
              <template #prepend>
                <v-icon icon="mdi-speedometer" size="16" />
              </template>
              <v-list-item-title class="text-caption">测试该账号</v-list-item-title>
            </v-list-item>
            <v-list-item density="compact" @click="openModels(actionAccount)" @contextmenu.stop.prevent>
              <template #prepend>
                <v-icon icon="mdi-file-tree-outline" size="16" />
              </template>
              <v-list-item-title class="text-caption">选择模型测试…</v-list-item-title>
            </v-list-item>
            <v-list-item density="compact" @click="toggleFromMenu" @contextmenu.stop.prevent>
              <template #prepend>
                <v-icon
                  :icon="actionAccount.schedulable ? 'mdi-circle-outline' : 'mdi-circle'"
                  size="16"
                />
              </template>
              <v-list-item-title class="text-caption">
                {{ actionAccount.schedulable ? "禁用该账号" : "启用该账号" }}
              </v-list-item-title>
            </v-list-item>
          </v-list>
          <!-- 账号列表 -->
          <v-list v-else density="compact" class="py-0 bg-transparent">
            <v-list-item
              v-for="a in accounts"
              :key="a.id"
              density="compact"
              @click="openAccountMenu(a)"
              @contextmenu.stop.prevent="openAccountMenu(a)"
            >
              <template #prepend>
                <v-icon
                  :icon="a.schedulable ? 'mdi-circle' : 'mdi-circle-outline'"
                  :color="a.schedulable ? 'success' : 'grey'"
                  size="9"
                />
              </template>
              <v-list-item-title class="text-caption">
                {{ a.name }}
                <v-icon
                  v-if="a.status === 'error'"
                  icon="mdi-alert"
                  size="10"
                  color="warning"
                  class="ml-1"
                  style="vertical-align: baseline"
                />
              </v-list-item-title>
              <template #append>
                <v-progress-circular
                  v-if="testingIds.has(a.id)"
                  indeterminate
                  size="12"
                  width="2"
                  class="ml-1"
                />
                <template v-else-if="results[String(a.id)]">
                  <span
                    v-if="results[String(a.id)]!.success && results[String(a.id)]!.first_token_ms != null"
                    :class="ftColor(results[String(a.id)]!.first_token_ms!)"
                    class="text-caption font-weight-medium ml-1"
                    :title="`模型：${results[String(a.id)]!.model || '默认'} · 总耗时 ${results[String(a.id)]!.total_ms ?? '—'} ms`"
                  >
                    {{ results[String(a.id)]!.first_token_ms }}ms
                  </span>
                  <v-icon
                    v-else-if="!results[String(a.id)]!.success"
                    icon="mdi-close-circle"
                    size="12"
                    color="error"
                    class="ml-1"
                    :title="results[String(a.id)]!.error ?? '测试失败'"
                  />
                </template>
                <v-chip
                  v-if="a.rate_limited"
                  size="x-small"
                  label
                  color="warning"
                  variant="tonal"
                  class="ml-1"
                >
                  限流
                </v-chip>
              </template>
            </v-list-item>
          </v-list>
        </div>

        <v-divider />
        <v-list density="compact" class="py-0 bg-transparent tray-actions">
          <v-list-item density="compact" @click="testAll">
            <template #prepend>
              <v-icon icon="mdi-speedometer" size="16" />
            </template>
            <v-list-item-title class="text-caption">测试全部账号</v-list-item-title>
          </v-list-item>
          <v-list-item density="compact" @click="reload">
            <template #prepend>
              <v-icon icon="mdi-refresh" size="16" />
            </template>
            <v-list-item-title class="text-caption">刷新账号列表</v-list-item-title>
          </v-list-item>
          <v-list-item density="compact" @click="openMain">
            <template #prepend>
              <v-icon icon="mdi-window-open" size="16" />
            </template>
            <v-list-item-title class="text-caption">显示主窗口</v-list-item-title>
          </v-list-item>
          <v-list-item density="compact" @click="quit">
            <template #prepend>
              <v-icon icon="mdi-exit-to-app" size="16" />
            </template>
            <v-list-item-title class="text-caption">退出</v-list-item-title>
          </v-list-item>
        </v-list>

      </v-card>
    </v-main>
  </v-app>
</template>

<style>
/* 透明窗口样式只作用于托盘菜单文档（.s2a-tray-doc 由脚本注入），避免污染主窗口 */
html.s2a-tray-doc,
html.s2a-tray-doc body,
html.s2a-tray-doc #app,
html.s2a-tray-doc .v-application {
  background: transparent !important;
  overflow: hidden;
}
/* 卡片铺满窗口，无外边距；阴影会贴边裁掉，改用描边；
   macOS 弹出菜单质感：12px 圆角 + 发丝边框（窗口透明，圆角外露出桌面） */
.tray-wrap {
  padding: 0 !important;
}
.tray-menu-card {
  background: rgba(250, 250, 252, 0.97);
  /* 窗口高度已由 Rust 按内容估算，卡片贴合窗口即可 */
  max-height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid rgba(0, 0, 0, 0.1);
  border-radius: 12px;
}
.tray-menu-card .v-divider {
  border-color: rgba(0, 0, 0, 0.08) !important;
}
/* 顶部单行：左侧可点击区与右侧用量区互为兄弟元素，事件互不影响（28px 固定高） */
.tray-header {
  flex: none;
  height: 28px;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 10px 0 6px;
  user-select: none;
}
/* 左侧点击区：切换分组/返回；标题过长截断，不挤压右侧用量 */
.header-btn {
  display: flex;
  align-items: center;
  gap: 3px;
  min-width: 0;
  flex: 0 1 auto;
  margin: 0;
  padding: 3px 7px;
  border: none;
  border-radius: 6px;
  background: transparent;
  font-family: inherit;
  font-size: 12px;
  font-weight: 600;
  color: rgba(0, 0, 0, 0.75);
  cursor: pointer;
  transition: background 0.15s var(--ease-in-out, ease);
}
.header-btn:hover {
  background: rgba(0, 0, 0, 0.05);
}
.header-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 右侧用量：非交互，固定不换行，点击不会触发左侧分组切换 */
.usage-stats {
  flex: none;
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
  font-size: 11px;
  color: rgba(0, 0, 0, 0.5);
  font-variant-numeric: tabular-nums;
  user-select: none;
  cursor: default;
}
/* 账号多时只允许账号区域内部滚动，底部操作区固定不被压缩 */
.tray-actions {
  flex: none;
}
/* 紧凑行高：所有菜单行统一 32px、小号图标（Rust 侧按 32px/行估算窗口高度，勿改） */
.tray-list .v-list-item,
.tray-actions .v-list-item {
  --v-list-item-one-line-height: 32px;
  min-height: 32px;
  border-radius: 7px;
  margin: 0 2px;
}
.tray-list .v-list-item .v-list-item-title,
.tray-actions .v-list-item .v-list-item-title {
  font-size: 12px !important;
  color: rgba(0, 0, 0, 0.85);
}
/* 行悬浮/涟漪高亮随行圆角 */
.tray-list .v-list-item__overlay,
.tray-actions .v-list-item__overlay {
  border-radius: 7px;
}
/* 底部操作区图标弱化为次级灰 */
.tray-actions .v-list-item .v-icon {
  color: rgba(0, 0, 0, 0.5);
}
.tray-list {
  position: relative;
  overflow-y: auto;
  min-height: 0;
  flex: 1 1 auto;
}
/* 加载条悬浮在列表顶部、不占布局：避免每次刷新让窗口高度抖动 2px */
.tray-list > .v-progress-linear {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  z-index: 1;
}
</style>
