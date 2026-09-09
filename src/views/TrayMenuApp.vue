<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AccountBrief, AppConfig, KeyBrief, KeyUsageToday, TestResult } from "../types";

const accounts = ref<AccountBrief[]>([]);
const loading = ref(false);
const groupName = ref("");
const groups = ref<{ id: number; name: string }[]>([]);
const currentGroupId = ref<number | null>(null);
/** 菜单内容模式：组 = 首行当前分组 + 账号列表；key = API Key 列表 */
const mode = ref<"groups" | "keys">("groups");
const keys = ref<KeyBrief[]>([]);
/** 子菜单窗口展开的账号（行高亮用）；分组级联展开时为 null，用 groupsOpen 指示。
 * 分组选择也在独立子菜单窗口里，主菜单不再切视图、高度不抖 */
const expandedId = ref<number | null>(null);
const groupsOpen = ref(false);
/** 子菜单窗口展开的 key（行高亮用，与账号 id 可能重叠，独立存放） */
const keysOpenId = ref<number | null>(null);
let closeTimer: number | null = null;
const keyUsage = ref<KeyUsageToday | null>(null);
/** 最近测试结果（后端缓存，主窗口测过的也能显示） */
const results = ref<Record<string, TestResult>>({});
const testingIds = ref(new Set<number>());
/** Key 测试结果与进行态（key id 与账号 id 可能重叠，独立存放） */
const keyResults = ref<Record<string, TestResult>>({});
const keyTestingIds = ref(new Set<number>());

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
/** 菜单最小高度：10 行账号免滚动 + 实测铬（顶栏/分隔线/底部操作/边框），
 * 内容再少也不缩成一条缝，底部操作区永远贴着菜单底边 */
const MIN_ROWS = 10;
const ROW_H = 32;
async function fitToContent(force = false) {
  const card = document.querySelector<HTMLElement>(".tray-menu-card");
  const list = card?.querySelector<HTMLElement>(".tray-list");
  if (!card || !list) return;
  let chromeH = 0;
  for (const child of Array.from(card.children)) {
    const el = child as HTMLElement;
    if (!el.classList.contains("tray-list")) chromeH += el.offsetHeight;
  }
  chromeH += card.offsetHeight - card.clientHeight; // 边框等盒模型外扩
  // 实测内容与下限取大：短列表留白（操作区置底）、长列表由列表区内滚吸收
  const h = Math.ceil(Math.max(list.scrollHeight + chromeH, chromeH + MIN_ROWS * ROW_H));
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

/** 悬停提示透明度跟随菜单不透明度设置（×85%），读配置即时生效 */
async function applyTipAlpha() {
  try {
    const cfg = await invoke<AppConfig>("get_config");
    const alpha = Math.min(1, Math.max(0, cfg.menu_opacity * 0.85));
    document.documentElement.style.setProperty("--tip-alpha", alpha.toFixed(3));
  } catch {
    // 忽略，保持默认
  }
}

/** 顶部组|key 切换：只换列表内容，高度下限兜底（短了留白、长了内滚） */
function setMode(m: "groups" | "keys") {
  if (mode.value === m) return;
  closeSubmenu();
  mode.value = m;
}

/** key 是否启用（status 非 active 视为禁用，行置灰） */
function isKeyEnabled(k: KeyBrief): boolean {
  return k.status === "active";
}

/** 顶部左侧区：在主菜单旁另起独立窗口级联分组列表（主菜单定高不切视图） */
function openGroupsSubmenu(rowTop: number) {
  cancelClose();
  expandedId.value = null;
  keysOpenId.value = null;
  groupsOpen.value = true;
  // 与账号子菜单共用独立窗口，内容由子菜单窗口自行渲染
  invoke("show_groups_submenu", { rowTop, height: 120 }).catch(() => {
    groupsOpen.value = false;
  });
}

/** 顶部额度点击：仅强制刷新即时用量，不碰账号列表（菜单不抖、不关子菜单）。
 * 旋转图标保底展示 1000ms：接口太快会导致图标一闪而过，反而像闪烁 */
const USAGE_MIN_SPIN_MS = 1000;
const reloadingUsage = ref(false);
const usageFlash = ref(false);
let flashTimer: number | null = null;
async function refreshUsage() {
  if (reloadingUsage.value) return;
  reloadingUsage.value = true;
  const t0 = Date.now();
  try {
    keyUsage.value = await invoke<KeyUsageToday | null>("get_usage_today", {
      force: true,
    });
  } catch {
    // 静默，旧值保留
  } finally {
    const wait = USAGE_MIN_SPIN_MS - (Date.now() - t0);
    if (wait > 0) await new Promise<void>((r) => window.setTimeout(r, wait));
    reloadingUsage.value = false;
    // 转完再闪一下，明确告知已更新为即时值
    usageFlash.value = false;
    if (flashTimer !== null) window.clearTimeout(flashTimer);
    requestAnimationFrame(() => {
      usageFlash.value = true;
      flashTimer = window.setTimeout(() => {
        usageFlash.value = false;
        flashTimer = null;
      }, 650);
    });
  }
}

/** 重新加载菜单数据；refreshUsage=true 绕过后端缓存强制刷新顶部额度 */
async function reload(refreshUsage = false) {
  closeSubmenu();
  loading.value = true;
  // 用量查询与账号列表并行，不阻塞主内容加载
  const usageP = invoke<KeyUsageToday | null>("get_usage_today", {
    force: refreshUsage,
  }).catch(() => null);
  const resultsP = invoke<Record<string, TestResult>>("get_last_results").catch(() => ({}));
  try {
    accounts.value = await invoke<AccountBrief[]>("list_accounts", { groupId: null });
    groups.value = await invoke<{ id: number; name: string }[]>("list_groups");
    const cfg = await invoke<{ group_id: number | null }>("get_config");
    currentGroupId.value = cfg.group_id;
    groupName.value = groups.value.find((g) => g.id === cfg.group_id)?.name ?? "全部账号";
    keys.value = await invoke<KeyBrief[]>("list_keys");
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

/** key 模式底部刷新：只刷 Key 列表（-loading 转圈复用顶部进度条） */
async function refreshKeysList() {
  if (loading.value) return;
  loading.value = true;
  try {
    keys.value = await invoke<KeyBrief[]>("list_keys");
  } catch {
    // 静默
  } finally {
    loading.value = false;
    scheduleFit();
  }
}

/** 底部刷新按当前模式走 */
function onRefreshAction() {
  if (mode.value === "keys") void refreshKeysList();
  else void reload(true);
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

function cancelClose() {
  if (closeTimer !== null) {
    window.clearTimeout(closeTimer);
    closeTimer = null;
  }
}

/** 对齐 macOS 原生菜单的三段式动效：
 * 展开 100ms（悬停意图，扫过不弹；Windows 调优共识值，macOS 近乎即开）；
 * 行间切换 200ms（安全三角的计时器近似：子菜单已展开时划到别行不立刻抢走，
 *  落进子菜单即取消，斜滑基本不断连）；
 * 收起 180ms 跨窗口桥接；关闭保持即时（原子菜单点选/点消不拖泥带水） */
const HOVER_OPEN_DELAY = 100;
const SWITCH_DELAY = 200;
let openTimer: number | null = null;
let pendingFire: (() => void) | null = null;

function cancelOpen() {
  if (openTimer !== null) {
    window.clearTimeout(openTimer);
    openTimer = null;
  }
  pendingFire = null;
}

/** 是否切到另一路子菜单（含同类不同行），是则走切换延迟 */
function isSwitchTarget(kind: "account" | "key" | "groups", id: number | null = null): boolean {
  if (kind !== "account" && expandedId.value !== null) return true;
  if (kind !== "key" && keysOpenId.value !== null) return true;
  if (kind !== "groups" && groupsOpen.value) return true;
  if (kind === "account" && expandedId.value !== null && expandedId.value !== id) return true;
  if (kind === "key" && keysOpenId.value !== null && keysOpenId.value !== id) return true;
  return false;
}

/** 悬停意图：停留才执行，快速扫过不弹，避免窗口反复横跳 */
function scheduleOpen(fire: () => void, delay = HOVER_OPEN_DELAY) {
  cancelOpen();
  pendingFire = fire;
  openTimer = window.setTimeout(() => {
    openTimer = null;
    const f = pendingFire;
    pendingFire = null;
    f?.();
  }, delay);
}

/** 立即展开子菜单窗口（点击走即时路径，不经过悬停延迟） */
function openSubmenu(kind: "account" | "key", id: number, rowTop: number) {
  // 三路互斥：新开一路即复位另两路的高亮与点击语义
  groupsOpen.value = false;
  expandedId.value = kind === "account" ? id : null;
  keysOpenId.value = kind === "key" ? id : null;
  // 子菜单窗口不可聚焦、不抢焦点；菜单内容由子菜单窗口按 kind + id 自取
  // 初始高度按两项菜单估算，随后子菜单 fit 按实测内容校正
  invoke("show_submenu", { kind, targetId: id, rowTop, height: 80 }).catch(() => {
    expandedId.value = null;
    keysOpenId.value = null;
  });
}

/** 立即收起子菜单窗口（账号操作 / 分组选择 / Key 操作共用） */
function closeSubmenu() {
  cancelClose();
  cancelOpen();
  expandedId.value = null;
  groupsOpen.value = false;
  keysOpenId.value = null;
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
  scheduleOpen(() => openSubmenu("account", a.id, rowTop), isSwitchTarget("account", a.id) ? SWITCH_DELAY : HOVER_OPEN_DELAY);
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
  openSubmenu("account", a.id, anchor ? rowTopOf(anchor) : 96);
}

/** 组模式首行（当前分组）：悬停/点击级联分组列表，与账号行同套意图与桥接 */
function onGroupRowEnter(e: Event) {
  cancelClose();
  if (groupsOpen.value) {
    cancelOpen();
    return;
  }
  const anchor = e.currentTarget as HTMLElement | null;
  const rowTop = anchor ? rowTopOf(anchor) : 32;
  scheduleOpen(() => openGroupsSubmenu(rowTop), isSwitchTarget("groups") ? SWITCH_DELAY : HOVER_OPEN_DELAY);
}

function toggleGroupRow(e: Event) {
  cancelOpen();
  if (groupsOpen.value) {
    closeSubmenu();
    return;
  }
  cancelClose();
  const anchor = e.currentTarget as HTMLElement | null;
  openGroupsSubmenu(anchor ? rowTopOf(anchor) : 32);
}

/** key 行：悬停/点击级联 Key 操作菜单（测试/选模型/启停） */
function onKeyEnter(k: KeyBrief, e: Event) {
  cancelClose();
  if (keysOpenId.value === k.id) {
    cancelOpen();
    return;
  }
  const anchor = e.currentTarget as HTMLElement | null;
  const rowTop = anchor ? rowTopOf(anchor) : 96;
  scheduleOpen(() => openSubmenu("key", k.id, rowTop), isSwitchTarget("key", k.id) ? SWITCH_DELAY : HOVER_OPEN_DELAY);
}

function toggleKey(k: KeyBrief, e: Event) {
  cancelOpen();
  if (keysOpenId.value === k.id) {
    closeSubmenu();
    return;
  }
  cancelClose();
  const anchor = e.currentTarget as HTMLElement | null;
  openSubmenu("key", k.id, anchor ? rowTopOf(anchor) : 96);
}

/** 列表滚动时行列错位，直接收起避免子菜单窗口悬空 */
function onListScroll() {
  if (expandedId.value !== null || groupsOpen.value || keysOpenId.value !== null) closeSubmenu();
}

/** 首 token 时长着色：阈值与主窗口一致；小字用 HIG 加深版文本色（design/accounts.html） */
function ftColor(ms: number): string {
  if (ms < 2000) return "s2a-text-success";
  if (ms < 5000) return "s2a-text-warning";
  return "s2a-text-danger";
}

async function quit() {
  await invoke("quit_app");
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    if (expandedId.value !== null || groupsOpen.value || keysOpenId.value !== null) closeSubmenu();
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
      void applyTipAlpha();
      // 每次弹出都回到账号列表：Rust 侧隐藏（再右键收起/失焦）不经过前端 hide()，
      // 子菜单展开态会残留，重开时收起（原生菜单行为）
      closeSubmenu();
      // 原生菜单式淡入：窗口是 show/hide 复用不重挂载，这里手动重播
      const cardEl = document.querySelector<HTMLElement>(".tray-menu-card");
      if (cardEl) {
        cardEl.classList.remove("menu-enter");
        void cardEl.offsetWidth;
        cardEl.classList.add("menu-enter");
      }
      // 每次弹出后端都会先按估算高度摆放窗口，需强制按当前内容重新贴合定位
      scheduleFit(true);
      void reload();
    })
  );
  // 子菜单窗口悬停状态：滑入取消收起（含待切换，安全三角的落点侧），滑出延迟收起
  unlistens.push(
    await listen<{ inside: boolean }>("submenu-hover", (e) => {
      if (e.payload.inside) {
        cancelClose();
        cancelOpen();
      } else scheduleClose();
    })
  );
  // 子菜单窗口发起的测速：行内转圈与结果回填（账号/key 分账存放，id 可能重叠）
  unlistens.push(
    await listen<{ kind: string; accountId: number }>("submenu-test-start", (e) => {
      (e.payload.kind === "key" ? keyTestingIds : testingIds).value.add(e.payload.accountId);
    })
  );
  unlistens.push(
    await listen<{ kind: string; accountId: number; result: TestResult | null }>(
      "submenu-tested",
      (e) => {
        const isKey = e.payload.kind === "key";
        (isKey ? keyTestingIds : testingIds).value.delete(e.payload.accountId);
        if (e.payload.result) {
          if (isKey) {
            keyResults.value = { ...keyResults.value, [String(e.payload.accountId)]: e.payload.result };
          } else {
            results.value = { ...results.value, [String(e.payload.accountId)]: e.payload.result };
          }
        }
      }
    )
  );
  // 子菜单窗口切换用量统计源后：收起并强制刷新顶部额度（转圈+闪光反馈一致）
  unlistens.push(
    await listen("submenu-usage-changed", () => {
      closeSubmenu();
      void refreshUsage();
    })
  );
  // 子菜单窗口切换 Key 启停后：收起并用事件带回的新列表同步行状态，不再多拉一次
  unlistens.push(
    await listen<{ keys: KeyBrief[] }>("submenu-key-toggled", (e) => {
      closeSubmenu();
      if (Array.isArray(e.payload.keys)) keys.value = e.payload.keys;
      scheduleFit();
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
  void applyTipAlpha();
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
        <!-- 顶部一行：左侧组|key 切换决定列表内容，右侧当天用量 -->
        <div class="tray-header">
          <div class="mode-seg" role="tablist" aria-label="菜单内容切换">
            <button
              type="button"
              role="tab"
              class="mode-seg-btn"
              :class="{ 'mode-seg-btn--active': mode === 'groups' }"
              :aria-selected="mode === 'groups'"
              @click="setMode('groups')"
            >
              Group
            </button>
            <button
              type="button"
              role="tab"
              class="mode-seg-btn"
              :class="{ 'mode-seg-btn--active': mode === 'keys' }"
              :aria-selected="mode === 'keys'"
              @click="setMode('keys')"
            >
              Key
            </button>
          </div>
          <v-tooltip
            v-if="keyUsage"
            :text="usageTitle"
            content-class="apple-tip"
            location="top"
            :open-delay="300"
          >
            <template #activator="{ props }">
              <button
                v-bind="props"
                type="button"
                class="usage-stats"
                :class="{ 'usage-flash': usageFlash }"
                :disabled="reloadingUsage"
                @click="refreshUsage"
              >
                <v-icon
                  :icon="reloadingUsage ? 'mdi-refresh' : 'mdi-chart-areaspline'"
                  size="12"
                  color="primary"
                  :class="{ 'spin-icon': reloadingUsage }"
                />
                今日 {{ fmtCost(keyUsage.cost) }} · {{ fmtM(keyUsage.total_tokens) }}
              </button>
            </template>
          </v-tooltip>
        </div>
        <v-divider />

        <div class="tray-list" @scroll="onListScroll">
          <v-progress-linear v-if="loading || reloadingUsage" indeterminate color="primary" height="2" />
          <template v-if="mode === 'groups'">
            <!-- 首行：当前分组，二级菜单切换分组；余下为该组账号，行为与之前一致 -->
            <v-list density="compact" class="py-0 bg-transparent">
              <v-list-item
                density="compact"
                :active="groupsOpen"
                @mouseenter="onGroupRowEnter($event)"
                @mouseleave="onRowLeave()"
                @click="toggleGroupRow($event)"
                @contextmenu.stop.prevent="toggleGroupRow($event)"
              >
                <template #prepend>
                  <v-icon icon="mdi-account-group" size="14" color="primary" />
                </template>
                <v-list-item-title class="text-caption font-weight-medium">
                  {{ groupName }}
                </v-list-item-title>
                <template #append>
                  <v-icon icon="mdi-chevron-right" size="14" class="submenu-hint" />
                </template>
              </v-list-item>
            </v-list>
            <!-- 原生菜单式分隔：当前分组与账号列表是不同类目 -->
            <v-divider />
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
          </template>
          <template v-else>
            <!-- key 列表：只有启用/禁用两种状态，悬停或点击行级联 Key 操作菜单 -->
            <v-list density="compact" class="py-0 bg-transparent">
              <v-list-item
                v-for="k in keys"
                :key="k.id"
                density="compact"
                :active="keysOpenId === k.id"
                @mouseenter="onKeyEnter(k, $event)"
                @mouseleave="onRowLeave()"
                @click="toggleKey(k, $event)"
                @contextmenu.stop.prevent="toggleKey(k, $event)"
              >
                <v-list-item-title class="text-caption" :class="{ 'text-disabled': !isKeyEnabled(k) }">
                  {{ k.name }}
                </v-list-item-title>
                <template #append>
                  <v-progress-circular
                    v-if="keyTestingIds.has(k.id)"
                    indeterminate
                    color="primary"
                    size="12"
                    width="2"
                    class="ml-1"
                  />
                  <template v-else-if="keyResults[String(k.id)]">
                    <v-tooltip
                      v-if="keyResults[String(k.id)]!.success && keyResults[String(k.id)]!.first_token_ms != null"
                      :text="`模型：${keyResults[String(k.id)]!.model || '默认'} · 总耗时 ${keyResults[String(k.id)]!.total_ms ?? '—'} ms`"
                      content-class="apple-tip"
                      location="top"
                      :open-delay="300"
                    >
                      <template #activator="{ props }">
                        <span
                          v-bind="props"
                          :class="ftColor(keyResults[String(k.id)]!.first_token_ms!)"
                          class="text-caption font-weight-medium ml-1"
                        >
                          {{ keyResults[String(k.id)]!.first_token_ms }}ms
                        </span>
                      </template>
                    </v-tooltip>
                    <v-icon
                      v-else-if="!keyResults[String(k.id)]!.success"
                      icon="mdi-close-circle"
                      size="12"
                      color="error"
                      class="ml-1"
                      :title="keyResults[String(k.id)]!.error ?? '测试失败'"
                    />
                  </template>
                  <v-icon icon="mdi-chevron-right" size="14" class="submenu-hint ml-1" />
                </template>
              </v-list-item>
              <div v-if="!loading && !keys.length" class="text-caption text-disabled pa-3">
                暂无 API Key
              </div>
            </v-list>
          </template>
        </div>

        <v-divider />
        <v-list density="compact" class="py-0 bg-transparent tray-actions">
          <v-list-item density="compact" @click="onRefreshAction">
            <template #prepend>
              <v-icon icon="mdi-refresh" size="16" />
            </template>
            <v-list-item-title class="text-caption">刷新</v-list-item-title>
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
/* 托盘窗口激活时 WebView 会把焦点交给 body，全局 :focus-visible 蓝描边
   会在菜单外围画出一圈蓝框；托盘菜单为纯鼠标交互，这里禁掉（不影响主窗口） */
html.s2a-tray-doc:focus-visible,
html.s2a-tray-doc body:focus-visible {
  outline: none;
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
  /* 卡片永远铺满窗口：窗口高度由 fit 决定（内容/下限），操作区始终贴菜单底边 */
  height: 100vh;
  max-height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid rgba(0, 0, 0, 0.08);
  /* 8px 与 DWM 窗口圆角（DWMWCP_ROUND）一致，窗口与卡片边缘严丝合缝 */
  border-radius: 8px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55);
}
/* 主菜单弹出淡入（macOS 菜单式；关闭保持即时，不断连） */
.menu-enter {
  animation: tray-in 0.12s ease-out;
}
@keyframes tray-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}
@media (prefers-reduced-motion: reduce) {
  .menu-enter {
    animation: none;
  }
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
  /* Vuetify 分隔线自带 0.12 透明度，会把浅色再压到隐形，这里复位为实线发丝边 */
  border-color: rgba(0, 0, 0, 0.1) !important;
  opacity: 1 !important;
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
/* 左侧组|key 切换：macOS 分段控件迷你版，决定下方列表内容 */
.mode-seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: 7px;
  background: rgba(118, 118, 128, 0.12);
  user-select: none;
}
.mode-seg-btn {
  min-width: 34px;
  height: 20px;
  padding: 0 8px;
  border: none;
  border-radius: 5px;
  background: transparent;
  font-family: inherit;
  font-size: 11px;
  font-weight: 500;
  color: rgba(0, 0, 0, 0.55);
  cursor: pointer;
}
.mode-seg-btn--active {
  background: rgba(255, 255, 255, 0.95);
  color: rgba(0, 0, 0, 0.85);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
}
/* 右侧用量：非交互，固定不换行，点击不会触发左侧分组切换 */
.usage-stats {
  flex: none;
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 5px;
  border: none;
  border-radius: 6px;
  background: transparent;
  font-family: inherit;
  white-space: nowrap;
  font-size: 11px;
  color: rgba(0, 0, 0, 0.5);
  font-variant-numeric: tabular-nums;
  user-select: none;
  cursor: pointer;
}
.usage-stats:hover:not(:disabled) {
  background: rgba(0, 0, 0, 0.05);
}
.usage-stats:disabled {
  cursor: default;
}
/* 刷新中：图标持续旋转，明确“正在取数” */
@keyframes usage-spin {
  to {
    transform: rotate(360deg);
  }
}
.spin-icon {
  animation: usage-spin 0.9s linear infinite;
}
/* 到值后：蓝色辉光闪一下，明确“已更新为即时值” */
@keyframes usage-flash {
  0% {
    background: rgba(0, 122, 255, 0.16);
  }
  100% {
    background: transparent;
  }
}
.usage-flash {
  animation: usage-flash 0.65s ease-out;
}
@media (prefers-reduced-motion: reduce) {
  .spin-icon,
  .usage-flash {
    animation: none;
  }
}
/* 账号多时只允许账号区域内部滚动，底部操作区固定不被压缩 */
.tray-actions {
  flex: none;
}
/* 紧凑行高：所有菜单行统一 32px、小号图标（Rust 侧按 32px/行估算窗口高度，勿改）；
   prepend 间距收紧到 8px（Vuetify 图标后 spacer 默认 32px，过宽不像原生菜单） */
.tray-list .v-list-item,
.tray-actions .v-list-item {
  --v-list-item-one-line-height: 32px;
  --v-list-prepend-gap: 8px;
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
