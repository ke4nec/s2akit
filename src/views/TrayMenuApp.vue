<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AccountBrief, KeyUsageToday, TestResult } from "../types";

const accounts = ref<AccountBrief[]>([]);
const loading = ref(false);
const groupName = ref("");
const groups = ref<{ id: number; name: string }[]>([]);
const currentGroupId = ref<number | null>(null);
/** 子菜单窗口展开的账号（行高亮用）；分组级联展开时为 null，用 groupsOpen 指示。
 * 分组选择也在独立子菜单窗口里，主菜单不再切视图、高度不抖 */
const expandedId = ref<number | null>(null);
const groupsOpen = ref(false);
let closeTimer: number | null = null;
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
  closeSubmenu();
  void getCurrentWindow().hide();
}

// ---------- 窗口高度贴合内容 ----------
// 测量菜单真实内容高度回报后端（fit_menu）：窗口随之收缩并按锚点重定位，
// 菜单底角始终贴住右键点击位置。注意不能直接取卡片 scrollHeight：
// 账号区域是内部滚动的 flex 子项（overflow-y:auto + min-height:0），
// 窗口一旦收窄（如内容曾变矮），卡片 scrollHeight 就被钳住不再反映自然高度，
// 菜单只会缩小、再也长不回去。正确做法：非滚动铬（顶栏/分隔线/底部操作）
// 取 offsetHeight，账号列表取自身 scrollHeight（滚动容器该值恒为内容全高）。
let lastFitH = 0;
async function fitToContent(force = false) {
  const card = document.querySelector<HTMLElement>(".tray-menu-card");
  const list = card?.querySelector<HTMLElement>(".tray-list");
  if (!card || !list) return;
  let h = 0;
  for (const child of Array.from(card.children)) {
    const el = child as HTMLElement;
    h += el.classList.contains("tray-list") ? el.scrollHeight : el.offsetHeight;
  }
  h += card.offsetHeight - card.clientHeight; // 边框等盒模型外扩
  h = Math.ceil(h);
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

/** 顶部左侧区：在主菜单旁另起独立窗口级联分组列表（主菜单定高不切视图） */
function openGroupsSubmenu(rowTop: number) {
  cancelClose();
  expandedId.value = null;
  groupsOpen.value = true;
  // 与账号子菜单共用独立窗口，内容由子菜单窗口自行渲染
  invoke("show_groups_submenu", { rowTop, height: 120 }).catch(() => {
    groupsOpen.value = false;
  });
}

function onHeaderEnter(e: Event) {
  cancelClose();
  if (groupsOpen.value) {
    cancelOpen();
    return;
  }
  // 与账号行同样的悬停意图，避免鼠标路过顶栏时误弹
  const anchor = e.currentTarget as HTMLElement | null;
  const rowTop = anchor ? rowTopOf(anchor) : 32;
  scheduleOpen(() => openGroupsSubmenu(rowTop));
}

function onHeaderLeave() {
  cancelOpen();
  scheduleClose();
}

function onHeaderClick(e: Event) {
  cancelOpen();
  if (groupsOpen.value) {
    closeSubmenu();
    return;
  }
  cancelClose();
  const anchor = e.currentTarget as HTMLElement | null;
  openGroupsSubmenu(anchor ? rowTopOf(anchor) : 32);
}

/** 重新加载菜单数据；refreshUsage=true 绕过后端缓存强制刷新顶部额度 */
async function reload(refreshUsage = false) {
  closeSubmenu();
  loading.value = true;
  // 用量查询与账号列表并行，不阻塞主内容加载
  const usageP = invoke<KeyUsageToday | null>("get_key_usage_today", {
    force: refreshUsage,
  }).catch(() => null);
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
  if (g.id === currentGroupId.value) return;
  try {
    await invoke("select_group", { groupId: g.id });
  } catch {
    hide();
    return;
  }
  await reload();
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

function cancelClose() {
  if (closeTimer !== null) {
    window.clearTimeout(closeTimer);
    closeTimer = null;
  }
}

/** 悬停意图延迟：鼠标扫过账号列表时不立即弹子菜单，停留片刻才展开，避免闪烁。
 *  取 100ms：Windows MenuShowDelay 默认 400ms 公认拖沓，调优共识 100ms
 *  “跟手又不跳”（0 则扫过即弹太神经质）；macOS 原生子菜单更是悬停近乎即开，
 *  防抖主要靠收起侧的跨窗口桥接而非拉长展开延迟 */
const HOVER_OPEN_DELAY = 100;
let openTimer: number | null = null;
let pendingFire: (() => void) | null = null;

function cancelOpen() {
  if (openTimer !== null) {
    window.clearTimeout(openTimer);
    openTimer = null;
  }
  pendingFire = null;
}

/** 悬停意图：停留 HOVER_OPEN_DELAY 才执行，快速扫过不弹，避免窗口反复横跳 */
function scheduleOpen(fire: () => void) {
  cancelOpen();
  pendingFire = fire;
  openTimer = window.setTimeout(() => {
    openTimer = null;
    const f = pendingFire;
    pendingFire = null;
    f?.();
  }, HOVER_OPEN_DELAY);
}

/** 立即展开子菜单窗口（点击走即时路径，不经过悬停延迟） */
function openSubmenu(id: number, rowTop: number) {
  // 从分组子菜单直接划到账号行时收起分组态：顶栏高亮/点击语义随之复位
  groupsOpen.value = false;
  expandedId.value = id;
  // 子菜单窗口不可聚焦、不抢焦点；菜单内容由子菜单窗口按 accountId 自取
  invoke("show_submenu", { accountId: id, rowTop, height: 120 }).catch(() => {
    expandedId.value = null;
  });
}

/** 立即收起子菜单窗口（账号操作 / 分组选择共用） */
function closeSubmenu() {
  cancelClose();
  cancelOpen();
  expandedId.value = null;
  groupsOpen.value = false;
  invoke("hide_submenu").catch(() => {});
}

/** 延迟收起：给“从账号行斜滑入子菜单窗口”留出跨越窗口间隙的时间 */
function scheduleClose(delay = 180) {
  cancelClose();
  closeTimer = window.setTimeout(() => {
    closeTimer = null;
    closeSubmenu();
  }, delay);
}

/** 账号行相对主菜单卡片上沿的逻辑偏移（CSS px），后端据此纵向定位子菜单窗口 */
function rowTopOf(anchor: HTMLElement): number {
  const card = document.querySelector<HTMLElement>(".tray-menu-card");
  if (!card) return 96;
  return anchor.getBoundingClientRect().top - card.getBoundingClientRect().top;
}

/** 悬停账号行：在主菜单旁另起独立窗口级联二级菜单（主菜单宽度不变） */
function onRowEnter(a: AccountBrief, e: Event) {
  cancelClose();
  if (expandedId.value === a.id) {
    cancelOpen();
    return;
  }
  const anchor = e.currentTarget as HTMLElement | null;
  const rowTop = anchor ? rowTopOf(anchor) : 96;
  scheduleOpen(() => openSubmenu(a.id, rowTop));
}

function onRowLeave() {
  cancelOpen();
  scheduleClose();
}

/** 点击账号行：同样展开/收起（触控板点不准、悬停困难时可用），点击即时展开 */
function toggleSubmenu(a: AccountBrief, e: Event) {
  cancelOpen();
  if (expandedId.value === a.id) {
    closeSubmenu();
    return;
  }
  cancelClose();
  const anchor = e.currentTarget as HTMLElement | null;
  openSubmenu(a.id, anchor ? rowTopOf(anchor) : 96);
}

/** 列表滚动时行列错位，直接收起避免子菜单窗口悬空 */
function onListScroll() {
  if (expandedId.value !== null) closeSubmenu();
}

/** 首 token 时长着色：阈值与主窗口一致；小字用 HIG 加深版文本色（design/accounts.html） */
function ftColor(ms: number): string {
  if (ms < 2000) return "s2a-text-success";
  if (ms < 5000) return "s2a-text-warning";
  return "s2a-text-danger";
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
    if (expandedId.value !== null || groupsOpen.value) closeSubmenu();
    else hide();
  }
}

const unlistens: (() => void)[] = [];
let ro: ResizeObserver | null = null;
onMounted(async () => {
  // 在挂载后标记：script setup 顶层会在模块导入时执行，
  // 而主窗口同样会静态 import 本组件，写在顶层会污染主窗口的 documentElement
  document.documentElement.classList.add("s2a-tray-doc");
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
  unlistens.push(
    await listen("tray-menu-shown", () => {
      void invoke("menu_pong");
      // 每次弹出都回到账号列表：Rust 侧隐藏（再右键收起/失焦）不经过前端 hide()，
      // 子菜单展开态会残留，重开时收起（原生菜单行为）
      closeSubmenu();
      // 每次弹出后端都会先按估算高度摆放窗口，需强制按当前内容重新贴合定位
      scheduleFit(true);
      void reload();
    })
  );
  // 子菜单窗口悬停状态：滑入取消收起，滑出延迟收起（跨窗口不断连）
  unlistens.push(
    await listen<{ inside: boolean }>("submenu-hover", (e) => {
      if (e.payload.inside) cancelClose();
      else scheduleClose();
    })
  );
  // 子菜单窗口发起的测速：行内转圈与结果回填
  unlistens.push(
    await listen<{ accountId: number }>("submenu-test-start", (e) => {
      testingIds.value.add(e.payload.accountId);
    })
  );
  unlistens.push(
    await listen<{ accountId: number; result: TestResult | null }>("submenu-tested", (e) => {
      testingIds.value.delete(e.payload.accountId);
      if (e.payload.result) {
        results.value = { ...results.value, [String(e.payload.accountId)]: e.payload.result };
      }
    })
  );
  // 子菜单窗口点选分组：收起并切换（高度跟新数据走，浏览分组时不动）
  unlistens.push(
    await listen<{ groupId: number }>("submenu-pick-group", (e) => {
      const g = groups.value.find((x) => x.id === e.payload.groupId);
      closeSubmenu();
      if (g) void pickGroup(g);
    })
  );
  // 别处（子菜单/主窗口）启停账号后刷新列表，行首圆点即时更新
  unlistens.push(
    await listen<AccountBrief[]>("accounts-updated", (e) => {
      accounts.value = e.payload;
      scheduleFit();
    })
  );
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
  cancelClose();
  cancelOpen();
  unlistens.forEach((u) => u());
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
        <!-- 顶部一行：左侧分组区悬停/点击级联分组列表，右侧当前 key 当天用量仅悬停看明细 -->
        <div class="tray-header">
          <button
            class="header-btn"
            :class="{ 'header-btn--active': groupsOpen }"
            type="button"
            @mouseenter="onHeaderEnter($event)"
            @mouseleave="onHeaderLeave()"
            @click="onHeaderClick($event)"
          >
            <span class="header-title">{{ groupName }}</span>
            <v-icon icon="mdi-chevron-down" size="14" color="primary" />
          </button>
          <v-tooltip
            v-if="keyUsage"
            :text="usageTitle"
            content-class="apple-tip"
            location="top"
            :open-delay="300"
          >
            <template #activator="{ props }">
              <span v-bind="props" class="usage-stats">
                <v-icon icon="mdi-chart-areaspline" size="12" color="primary" />
                今日 {{ fmtCost(keyUsage.cost) }} · {{ fmtM(keyUsage.total_tokens) }}
              </span>
            </template>
          </v-tooltip>
        </div>
        <v-divider />

        <div class="tray-list" @scroll="onListScroll">
          <v-progress-linear v-if="loading" indeterminate color="primary" height="2" />
          <!-- 账号列表：悬停或点击账户行，在主菜单旁另起窗口级联二级菜单（主列表保持可见） -->
          <v-list density="compact" class="py-0 bg-transparent">
            <v-list-item
              v-for="a in accounts"
              :key="a.id"
              density="compact"
              :active="expandedId === a.id"
              @mouseenter="onRowEnter(a, $event)"
              @mouseleave="onRowLeave()"
              @click="toggleSubmenu(a, $event)"
              @contextmenu.stop.prevent="toggleSubmenu(a, $event)"
            >
              <template #prepend>
                <v-icon
                  :icon="a.schedulable ? 'mdi-circle' : 'mdi-circle-outline'"
                  :color="a.schedulable ? 'success' : '#C7C7CC'"
                  size="9"
                />
              </template>
              <v-list-item-title class="text-caption">
                {{ a.name }}
                <v-icon
                  v-if="a.status === 'error'"
                  icon="mdi-alert"
                  size="10"
                  color="error"
                  class="ml-1"
                  style="vertical-align: baseline"
                />
              </v-list-item-title>
              <template #append>
                <v-progress-circular
                  v-if="testingIds.has(a.id)"
                  indeterminate
                  color="primary"
                  size="12"
                  width="2"
                  class="ml-1"
                />
                <template v-else-if="results[String(a.id)]">
                  <v-tooltip
                    v-if="results[String(a.id)]!.success && results[String(a.id)]!.first_token_ms != null"
                    :text="`模型：${results[String(a.id)]!.model || '默认'} · 总耗时 ${results[String(a.id)]!.total_ms ?? '—'} ms`"
                    content-class="apple-tip"
                    location="top"
                    :open-delay="300"
                  >
                    <template #activator="{ props }">
                      <span
                        v-bind="props"
                        :class="ftColor(results[String(a.id)]!.first_token_ms!)"
                        class="text-caption font-weight-medium ml-1"
                      >
                        {{ results[String(a.id)]!.first_token_ms }}ms
                      </span>
                    </template>
                  </v-tooltip>
                  <v-tooltip
                    v-else-if="!results[String(a.id)]!.success"
                    :text="results[String(a.id)]!.error ?? '测试失败'"
                    content-class="apple-tip"
                    location="top"
                    :open-delay="300"
                  >
                    <template #activator="{ props }">
                      <v-icon
                        v-bind="props"
                        icon="mdi-close-circle"
                        size="12"
                        color="error"
                        class="ml-1"
                      />
                    </template>
                  </v-tooltip>
                </template>
                <span v-if="a.rate_limited" class="s2a-flag ml-1">限流</span>
                <v-icon icon="mdi-chevron-right" size="14" class="submenu-hint ml-1" />
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
          <v-list-item density="compact" @click="() => reload(true)">
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
  /* Apple 菜单质感：窗口级 Acrylic 毛玻璃打底，卡片只盖半透明底色；
     发丝边框 + 顶部内高光，系统阴影已在窗口配置关闭，不再叠边 */
  background: rgba(246, 246, 248, 0.72);
  /* 窗口高度已由 Rust 按内容估算，卡片贴合窗口即可 */
  max-height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid rgba(0, 0, 0, 0.08);
  /* 8px 与 DWM 窗口圆角（DWMWCP_ROUND）一致，窗口与卡片边缘严丝合缝 */
  border-radius: 8px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55);
}
/* 子菜单指示箭头：常态弱化，行展开/悬浮时加深 */
.submenu-hint {
  color: rgba(0, 0, 0, 0.3);
}
.v-list-item:hover .submenu-hint,
.v-list-item--active .submenu-hint {
  color: rgba(0, 0, 0, 0.55);
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
.header-btn:hover,
.header-btn--active {
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
