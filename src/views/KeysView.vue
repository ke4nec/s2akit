<script setup lang="ts">
import { computed, h, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { handleErr, refreshGroups, store } from "../store";
import type { KeyBrief, KeyUsageToday, ModelBrief, TestResult } from "../types";

/// 表头排序指示（Apple 风成对 chevron，与 AccountsView 同款）
function headerChevron(up: boolean, lit: boolean) {
  return h(
    "svg",
    {
      viewBox: "0 0 16 9",
      width: "8",
      height: "5",
      class: lit ? "s2a-si-on" : undefined,
      fill: "none",
      stroke: "currentColor",
      "stroke-width": "2.2",
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
    },
    [h("polyline", { points: up ? "3.5 6.8 8 2.8 12.5 6.8" : "3.5 2.8 8 6.8 12.5 2.8" })],
  );
}

const HeaderCell = (props: {
  column: { key: string | null; title?: string; sortable?: boolean };
  sortBy: ReadonlyArray<{ key: string | null; order?: boolean | "asc" | "desc" }>;
}) => {
  const active = props.sortBy?.find((s) => s.key === props.column.key);
  return h("span", { class: "s2a-th" }, [
    h("span", { class: active ? "s2a-th-title s2a-th--active" : "s2a-th-title" }, props.column.title),
    props.column.sortable
      ? h("span", { class: "s2a-si", "aria-hidden": "true" }, [
          headerChevron(true, active?.order === "asc"),
          headerChevron(false, active?.order === "desc"),
        ])
      : null,
  ]);
};

/// 状态列排序权重：启用 → 停用 → 其他（升降序沿此语义）
function statusRank(s: string): number {
  if (s === "active") return 0;
  if (s === "inactive") return 1;
  return 2;
}

const headers = [
  { title: "Key 名称", key: "name", sortable: true, width: 216 },
  {
    title: "状态",
    key: "status",
    sortable: true,
    width: 84,
    // Vuetify 表头 sort 收到的是列的值（status 字符串），按语义权重排序
    sort: (a: string, b: string) => statusRank(a) - statusRank(b),
  },
  { title: "调度", key: "enabled", sortable: true, width: 132 },
  { title: "首 token", key: "first_token", sortable: true, width: 104 },
  { title: "总耗时", key: "total", sortable: true, width: 96 },
  { title: "用量", key: "usageCost", sortable: true, width: 150 },
  { title: "操作", key: "actions", sortable: false, align: "end" as const, width: 150 },
];

interface KeyRow {
  id: number;
  name: string;
  secret: string;
  status: string;
  enabled: boolean;
  group: string;
  result?: TestResult;
  testing: boolean;
  first_token: number;
  total: number;
  /** 当日用量（后台静默填充）；用量列按费用排序，无数据排最后 */
  usage?: KeyUsageToday;
  usageCost: number;
}

const keys = ref<KeyBrief[]>([]);
const keyResults = ref<Record<number, TestResult>>({});
const keyTesting = ref(new Set<number>());
const loadingKeys = ref(false);
/** 各 Key 当日用量（后台静默填充，失败留空不断连） */
const keyUsageMap = ref<Record<number, KeyUsageToday>>({});

function groupNameOf(groupId: number | null): string {
  if (groupId == null) return "未分组";
  return store.groups.find((g) => g.id === groupId)?.name ?? "未分组";
}

const rows = computed<KeyRow[]>(() =>
  keys.value.map((k) => {
    const r = keyResults.value[k.id];
    const row: KeyRow = {
      id: k.id,
      name: k.name,
      secret: k.key,
      status: k.status,
      enabled: k.status === "active",
      group: groupNameOf(k.group_id),
      result: r,
      testing: keyTesting.value.has(k.id),
      // 排序键：无数据用极大值，保证升序时已测 Key 在前
      first_token: r?.first_token_ms ?? Number.MAX_SAFE_INTEGER,
      total: r?.total_ms ?? Number.MAX_SAFE_INTEGER,
      usage: keyUsageMap.value[k.id],
      usageCost: keyUsageMap.value[k.id]?.cost ?? Number.MAX_SAFE_INTEGER,
    };
    return row;
  }),
);

/** 金额格式化：保留两位小数 */
function fmtMoney(n: number): string {
  return `$${n.toFixed(2)}`;
}

/** token 量格式化：K/M 紧凑显示 */
function fmtTokens(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 1 : 2)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return `${n}`;
}

/** 用量副行：次数 · token 量 */
function usageSub(u: KeyUsageToday): string {
  return `${u.requests} 次 · ${fmtTokens(u.total_tokens)}`;
}

/** 用量列悬停明细（原主窗口右上角悬停同风格）：请求数、输入/输出/缓存、缓存率、均值与费用 */
function usageTip(row: KeyRow): string {
  const u = row.usage;
  if (!u) return "";
  const rate = u.total_tokens > 0 ? `${((u.cache_tokens / u.total_tokens) * 100).toFixed(1)}%` : "—";
  const avg = u.requests > 0 ? fmtTokens(Math.round(u.total_tokens / u.requests)) : "—";
  return `「${row.name}」今日 ${u.requests} 次请求 · 平均 ${avg} tok/次\n输入 ${fmtTokens(u.input_tokens)} · 输出 ${fmtTokens(u.output_tokens)} · 缓存 ${fmtTokens(u.cache_tokens)}\n缓存率 ${rate} · 共 ${fmtTokens(u.total_tokens)} · 费用 ${fmtMoney(u.cost)}`;
}

/// 页码窗口：页数不多时全量展示，页数多时收敛为 首页/尾页/当前±1，0 代表省略号
const itemsPerPage = ref(10);
const itemsPerPageOptions = [10, 20, 50, 100];

function pageItems(page: number, pageCount: number): number[] {
  if (pageCount <= 7) return Array.from({ length: pageCount }, (_, i) => i + 1);
  const shown = new Set<number>([1, pageCount, page - 1, page, page + 1]);
  const out: number[] = [];
  let prev = 0;
  for (const p of [...shown].filter((n) => n >= 1 && n <= pageCount).sort((a, b) => a - b)) {
    if (p - prev > 1) out.push(0);
    out.push(p);
    prev = p;
  }
  return out;
}

/// 行状态判定：启用 > 停用，驱动行首竖条与汇总计数
function rowState(item: KeyRow): "ok" | "off" {
  return item.enabled ? "ok" : "off";
}

/// 卡片头状态汇总
const stateCounts = computed(() => {
  const c = { on: 0, off: 0 };
  for (const r of rows.value) c[rowState(r) === "ok" ? "on" : "off"] += 1;
  return c;
});

/// 行右键弹出快捷菜单
function rowProps({ item }: { item: KeyRow }): Record<string, unknown> {
  return { onContextmenu: (e: MouseEvent) => openCtxMenu(e, item) };
}

/// 状态徽标配色：启用绿 / 其余灰
function statusBadgeClass(s: string): string {
  if (s === "active") return "s2a-tag--ok";
  return "s2a-tag--off";
}

/// 首 token 阈值着色：<2s 绿 / <5s 橙 / 其余红（文本用 HIG 加深色）
function firstTokenClass(r: TestResult): string {
  if (!r.success) return "s2a-num--bad";
  if (r.first_token_ms == null) return "";
  if (r.first_token_ms < 2000) return "s2a-num--ok";
  if (r.first_token_ms < 5000) return "s2a-num--mid";
  return "s2a-num--bad";
}

function statusText(s: string): string {
  if (s === "active") return "正常";
  if (s === "error") return "错误";
  if (s === "inactive") return "停用";
  return s;
}

/** 当前用量统计源 Key（null = 汇总全部）：托盘菜单顶部展示，与托盘子菜单共用同一后端状态 */
const usageKeyId = ref<number | null>(null);

async function refreshKeys() {
  loadingKeys.value = true;
  try {
    keys.value = await invoke<KeyBrief[]>("list_keys");
    usageKeyId.value = await invoke<number | null>("get_usage_key").catch(() => null);
    // 用量另起后台填充，不阻塞表格呈现
    void refreshKeysUsage();
  } catch (e) {
    handleErr(e);
  } finally {
    loadingKeys.value = false;
  }
}

async function refreshKeysUsage() {
  try {
    const list = await invoke<KeyUsageToday[]>("list_keys_usage");
    const map: Record<number, KeyUsageToday> = {};
    for (const u of list) map[u.key_id] = u;
    keyUsageMap.value = map;
  } catch {
    // 静默，单元格保持 —
  }
}

/** 指定默认 Key：切换托盘菜单顶部用量统计源，再点恢复汇总全部 */
async function onPickUsage() {
  const row = ctxRow.value;
  closeCtxMenu();
  if (!row || row.testing) return;
  const toId = usageKeyId.value === row.id ? null : row.id;
  try {
    await invoke("set_usage_key", { keyId: toId });
    usageKeyId.value = toId;
  } catch (e) {
    handleErr(e);
  }
}

async function onRefresh() {
  await refreshGroups();
  await refreshKeys();
}

function goSettings() {
  store.tab = "settings";
}

async function testKey(id: number, model?: string) {
  const k = keys.value.find((x) => x.id === id);
  if (!k || keyTesting.value.has(id)) return;
  keyTesting.value.add(id);
  try {
    const r = await invoke<TestResult>("tray_test_key", {
      keyId: k.id,
      keyName: k.name,
      keySecret: k.key,
      model: model ?? null,
    });
    keyResults.value = { ...keyResults.value, [id]: r };
  } catch (e) {
    handleErr(e);
  } finally {
    keyTesting.value.delete(id);
  }
}

async function toggleKey(id: number) {
  const k = keys.value.find((x) => x.id === id);
  if (!k || keyTesting.value.has(id)) return;
  try {
    keys.value = await invoke<KeyBrief[]>("set_key_enabled", {
      keyId: id,
      enabled: k.status !== "active",
    });
  } catch (e) {
    handleErr(e);
  }
}

// ---- 单 Key 模型选择测试 ----
const modelDialog = ref(false);
const modelKey = ref<KeyRow | null>(null);
const models = ref<ModelBrief[]>([]);
const loadingModels = ref(false);

async function openModelDialog(row: KeyRow) {
  modelKey.value = row;
  models.value = [];
  modelDialog.value = true;
  loadingModels.value = true;
  try {
    models.value = await invoke<ModelBrief[]>("get_key_models", { keySecret: row.secret });
  } catch (e) {
    modelDialog.value = false;
    handleErr(e);
  } finally {
    loadingModels.value = false;
  }
}

function testWithModel(model: ModelBrief) {
  if (!modelKey.value) return;
  modelDialog.value = false;
  void testKey(modelKey.value.id, model.id);
}

// ---- 行右键快捷菜单：选择模型测试 / 启用禁用 ----
const ctxMenu = ref(false);
const ctxRow = ref<KeyRow | null>(null);
const ctxX = ref(0);
const ctxY = ref(0);
const ctxRef = ref<HTMLElement | null>(null);

function openCtxMenu(e: MouseEvent, row: KeyRow) {
  e.preventDefault();
  e.stopPropagation();
  ctxRow.value = row;
  ctxMenu.value = false;
  // 先按光标定位，下 tick 按菜单实测尺寸钳入视口
  ctxX.value = e.clientX;
  ctxY.value = e.clientY;
  void nextTick(() => {
    ctxMenu.value = true;
    void nextTick(placeCtxMenu);
  });
}

/** 按菜单实测尺寸把右键菜单钳入视口，避免贴边时溢出 */
function placeCtxMenu() {
  const el = ctxRef.value;
  if (!el) return;
  ctxX.value = Math.max(8, Math.min(ctxX.value, window.innerWidth - el.offsetWidth - 8));
  ctxY.value = Math.max(8, Math.min(ctxY.value, window.innerHeight - el.offsetHeight - 8));
}

function closeCtxMenu() {
  ctxMenu.value = false;
  ctxRow.value = null;
  closeCtxSub();
  ctxModels.value = [];
}

function ctxToggle() {
  const row = ctxRow.value;
  closeCtxMenu();
  if (row && !row.testing) void toggleKey(row.id);
}

// ---- 模型二级子菜单：与托盘级联子菜单同款交互（悬停意图延迟、贴边翻转）----
const ctxSubOpen = ref(false);
const ctxSubX = ref(0);
const ctxSubY = ref(0);
/// 子菜单高度上限：min(320, 视口高-16)，保证在窗口内完整展开（底部时往上长）
const ctxSubMaxH = ref(320);
const ctxSubRef = ref<HTMLElement | null>(null);
const ctxModels = ref<ModelBrief[]>([]);
const ctxModelsLoading = ref(false);
/// 触发项左右边缘（视口坐标）：右侧空间不足时子菜单向左翻转
let subAnchorLeft = 0;
let subAnchorRight = 0;
let subTimer: number | undefined;

/// 悬停意图：停约 220ms 才展开，避免扫过时误触（同托盘子菜单）
function ctxSubHoverOpen(e: MouseEvent) {
  window.clearTimeout(subTimer);
  const anchor = e.currentTarget as HTMLElement;
  subTimer = window.setTimeout(() => showCtxSub(anchor), 220);
}

/// 悬停离开：给一小段缓冲期，容许斜向移入子菜单
function ctxSubHoverClose() {
  window.clearTimeout(subTimer);
  subTimer = window.setTimeout(closeCtxSub, 260);
}

function ctxSubHoverStay() {
  window.clearTimeout(subTimer);
}

/// 点击触发项立即展开/收起（悬停之外的显式路径）
function ctxSubClick(e: MouseEvent) {
  window.clearTimeout(subTimer);
  if (ctxSubOpen.value) {
    closeCtxSub();
    return;
  }
  showCtxSub(e.currentTarget as HTMLElement);
}

function showCtxSub(anchor: HTMLElement) {
  const row = ctxRow.value;
  if (!row || row.testing) return;
  const r = anchor.getBoundingClientRect();
  subAnchorLeft = r.left;
  subAnchorRight = r.right;
  ctxSubX.value = r.right + 4;
  ctxSubY.value = r.top - 4;
  ctxSubMaxH.value = Math.max(120, Math.min(320, window.innerHeight - 16));
  ctxSubOpen.value = true;
  void nextTick(placeCtxSub);
  // 模型列表按 Key 懒加载，列表为空时才请求（关闭菜单时清空）
  if (!ctxModels.value.length && !ctxModelsLoading.value) void loadCtxModels();
}

async function loadCtxModels() {
  const row = ctxRow.value;
  if (!row) return;
  ctxModelsLoading.value = true;
  try {
    ctxModels.value = await invoke<ModelBrief[]>("get_key_models", { keySecret: row.secret });
  } catch (e) {
    ctxSubOpen.value = false;
    handleErr(e);
  } finally {
    ctxModelsLoading.value = false;
    // 加载完成高度会变（loading 提示 → 实际列表），需重新贴边：底部时把子菜单往上长
    void nextTick(placeCtxSub);
  }
}

/// 子菜单贴边处理：右侧放不下翻到触发项左侧，纵向钳入视口
function placeCtxSub() {
  const el = ctxSubRef.value;
  if (!el) return;
  ctxSubX.value =
    subAnchorRight + el.offsetWidth > window.innerWidth - 8
      ? Math.max(8, subAnchorLeft - el.offsetWidth - 4)
      : subAnchorRight + 4;
  ctxSubY.value = Math.max(8, Math.min(ctxSubY.value, window.innerHeight - el.offsetHeight - 8));
}

function closeCtxSub() {
  window.clearTimeout(subTimer);
  ctxSubOpen.value = false;
}

/// 点击模型即以该模型测试，随后整组菜单收起
function ctxTestWith(m: ModelBrief) {
  const row = ctxRow.value;
  closeCtxMenu();
  if (row && !row.testing) void testKey(row.id, m.id);
}

function onCtxKey(e: KeyboardEvent) {
  if (e.key === "Escape" && ctxMenu.value) closeCtxMenu();
}

onMounted(() => {
  window.addEventListener("keydown", onCtxKey);
  // 未登录只展示提示条（模板内 v-alert），不拉数据，避免未授权错误打扰
  if (store.auth) {
    void refreshGroups();
    void refreshKeys();
  }
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onCtxKey);
  window.clearTimeout(subTimer);
});
</script>

<template>
  <div>
    <v-alert
      v-if="!store.auth"
      type="info"
      variant="tonal"
      density="compact"
      rounded="lg"
      class="mb-3"
      prominent
    >
      <template #prepend>
        <v-icon icon="mdi-login" />
      </template>
      请先在设置页填写 sub2api 地址与管理员账号并登录。
      <template #append>
        <v-btn size="small" variant="tonal" @click="goSettings">前往设置</v-btn>
      </template>
    </v-alert>

    <div v-if="store.auth" class="table-card">
      <!-- 状态汇总：左侧启用/停用计数，右侧刷新 -->
      <div class="s2a-summary">
        <span class="s2a-sum">
          <i class="s2a-sum-dot s2a-dot--ok" aria-hidden="true"></i>正常 <b>{{ stateCounts.on }}</b>
        </span>
        <span class="s2a-sum">
          <i class="s2a-sum-dot s2a-dot--off" aria-hidden="true"></i>停用 <b>{{ stateCounts.off }}</b>
        </span>
        <div class="s2a-summary-tools">
          <v-tooltip text="刷新 Key 列表" content-class="apple-tip">
            <template #activator="{ props }">
              <v-btn
                v-bind="props"
                icon="mdi-refresh"
                variant="text"
                size="small"
                :loading="loadingKeys"
                @click="onRefresh"
              />
            </template>
          </v-tooltip>
        </div>
      </div>
      <v-data-table
        :headers="headers"
        :items="rows"
        :loading="loadingKeys"
        :row-props="rowProps"
        v-model:items-per-page="itemsPerPage"
        density="compact"
        hover
        no-data-text="暂无 API Key"
      >
        <!-- 表头：Apple 风成对 chevron 排序指示（自定义插槽替换默认图标，th 点击排序仍由 Vuetify 处理）；
             函数组件未声明 props，绑定需用 camelCase :sortBy，kebab 不会归一化 -->
        <template #header.name="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.status="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.enabled="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.first_token="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.total="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.usageCost="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>

        <template #item.name="{ item }">
          <v-tooltip :disabled="!item.result" max-width="420" content-class="apple-tip">
            <template #activator="{ props: tipProps }">
              <div v-bind="tipProps" style="min-width: 0">
                <span class="s2a-bar" :class="`s2a-bar--${rowState(item)}`" aria-hidden="true"></span>
                <div class="s2a-name">
                  {{ item.name }}
                  <v-icon
                    v-if="usageKeyId === item.id"
                    icon="mdi-chart-areaspline"
                    size="11"
                    color="primary"
                    class="ml-1"
                    style="vertical-align: baseline"
                    title="正在查看该 Key 额度"
                  />
                </div>
                <div class="s2a-meta">#{{ item.id }} · {{ item.group }}</div>
              </div>
            </template>
            <div v-if="item.result">
              <div v-if="item.result.success" class="text-success font-weight-medium">
                ✓ 测试成功
              </div>
              <div v-else class="text-error font-weight-medium">✗ 测试失败</div>
              <div>首 token：{{ item.result.first_token_ms ?? "—" }} ms</div>
              <div>总耗时：{{ item.result.total_ms ?? "—" }} ms</div>
              <div>模型：{{ item.result.model || "默认" }}</div>
              <div v-if="item.result.success && item.result.content_preview">
                回复：{{ item.result.content_preview }}
              </div>
              <div v-if="!item.result.success && item.result.error" class="text-error">
                错误：{{ item.result.error }}
              </div>
            </div>
          </v-tooltip>
        </template>

        <template #item.status="{ item }">
          <span class="s2a-tag" :class="statusBadgeClass(item.status)">{{ statusText(item.status) }}</span>
        </template>

        <template #item.enabled="{ item }">
          <div class="s2a-sw-wrap">
            <button
              type="button"
              class="s2a-sw"
              :class="{ 's2a-sw--on': item.enabled }"
              role="switch"
              :aria-checked="item.enabled"
              :aria-label="item.enabled ? '禁用该 Key' : '启用该 Key'"
              :disabled="item.testing"
              @click="toggleKey(item.id)"
            ></button>
          </div>
        </template>

        <template #item.first_token="{ item }">
          <span v-if="item.testing" class="s2a-skeleton" aria-label="测试中"></span>
          <template v-else-if="item.result">
            <span v-if="item.result.first_token_ms != null" class="s2a-num" :class="firstTokenClass(item.result)">
              {{ item.result.first_token_ms.toLocaleString() }}<span class="s2a-unit">ms</span>
            </span>
            <v-icon v-else-if="!item.result.success" color="error" size="small" icon="mdi-close-circle" />
            <span v-else class="s2a-none">—</span>
          </template>
          <span v-else class="s2a-none">—</span>
        </template>

        <template #item.total="{ item }">
          <span v-if="item.testing" class="s2a-skeleton" aria-label="测试中"></span>
          <span v-else-if="item.result?.total_ms != null" class="s2a-num">
            {{ item.result.total_ms.toLocaleString() }}<span class="s2a-unit">ms</span>
          </span>
          <span v-else class="s2a-none">—</span>
        </template>

        <template #item.usageCost="{ item }">
          <v-tooltip v-if="item.usage" :text="usageTip(item)" content-class="apple-tip">
            <template #activator="{ props }">
              <div v-bind="props">
                <div class="s2a-name">今日 {{ fmtMoney(item.usage.cost) }}</div>
                <div class="s2a-meta">{{ usageSub(item.usage) }}</div>
              </div>
            </template>
          </v-tooltip>
          <span v-else class="s2a-none">—</span>
        </template>

        <template #item.actions="{ item }">
          <v-btn
            size="small"
            variant="text"
            color="primary"
            :disabled="item.testing"
            @click="testKey(item.id)"
          >
            测试
          </v-btn>
          <v-tooltip text="选择模型测试" content-class="apple-tip">
            <template #activator="{ props }">
              <v-btn
                v-bind="props"
                size="small"
                icon="mdi-file-tree-outline"
                variant="text"
                color="primary"
                class="ml-1"
                :disabled="item.testing"
                @click="openModelDialog(item)"
              />
            </template>
          </v-tooltip>
        </template>

        <!-- 卡片底分页条：替换默认 footer -->
        <template #bottom="{
          itemsLength,
          itemsPerPage: perPage,
          page,
          pageCount,
          setPage,
          setItemsPerPage,
          prevPage,
          nextPage,
        }">
          <div v-if="itemsLength > 0" class="s2a-pager">
            <span class="s2a-pager-total">共 <b>{{ itemsLength }}</b> 个 Key</span>
            <div class="s2a-pager-right">
              <span class="s2a-pager-size">
                <span>每页</span>
                <v-menu scroll-strategy="close">
                  <template #activator="{ props: menuProps }">
                    <button v-bind="menuProps" type="button" class="s2a-pager-select">
                      {{ perPage }} 条
                      <v-icon icon="mdi-menu-down" size="12" />
                    </button>
                  </template>
                  <v-list density="compact">
                    <v-list-item
                      v-for="n in itemsPerPageOptions"
                      :key="n"
                      :active="n === perPage"
                      :title="`${n} 条`"
                      @click="setItemsPerPage(n)"
                    />
                  </v-list>
                </v-menu>
              </span>
              <div class="s2a-pager-nav">
                <button class="s2a-pg-btn" type="button" :disabled="page <= 1" title="上一页" @click="prevPage">
                  <v-icon icon="mdi-chevron-left" size="14" />
                </button>
                <template v-for="(p, i) in pageItems(page, pageCount)" :key="`${p}-${i}`">
                  <span v-if="p === 0" class="s2a-pg-ellipsis">…</span>
                  <button
                    v-else
                    class="s2a-pg-btn"
                    :class="{ 's2a-pg-cur': p === page }"
                    type="button"
                    @click="setPage(p)"
                  >
                    {{ p }}
                  </button>
                </template>
                <button
                  type="button"
                  class="s2a-pg-btn"
                  :disabled="page >= pageCount"
                  title="下一页"
                  @click="nextPage"
                >
                  <v-icon icon="mdi-chevron-right" size="14" />
                </button>
              </div>
            </div>
          </div>
        </template>
      </v-data-table>
    </div>

    <v-dialog v-model="modelDialog" max-width="520">
      <v-card rounded="xl" class="model-dialog">
        <div class="model-dialog-head">
          <div class="model-dialog-title">选择测试模型</div>
          <div v-if="modelKey" class="model-dialog-sub">{{ modelKey.name }}</div>
        </div>
        <v-progress-linear v-if="loadingModels" indeterminate color="primary" />
        <div class="model-dialog-body">
          <div v-if="!loadingModels && !models.length" class="text-body-2 text-medium-emphasis">
            该 Key 没有可用的模型列表
          </div>
          <v-list v-else-if="!loadingModels" density="compact" max-height="360" class="py-0 model-list">
            <v-list-item
              v-for="m in models"
              :key="m.id"
              :subtitle="m.id"
              @click="testWithModel(m)"
            >
              <template #prepend>
                <v-icon icon="mdi-chat-processing-outline" size="small" />
              </template>
              <v-list-item-title>{{ m.display_name || m.id }}</v-list-item-title>
            </v-list-item>
          </v-list>
        </div>
        <div class="model-dialog-foot">
          <v-btn variant="text" @click="modelDialog = false">取消</v-btn>
        </div>
      </v-card>
    </v-dialog>

    <!-- 行右键快捷菜单：跟随光标，点消（与托盘菜单一致，右键空白处收起）。
         必须 Teleport 到 body：App.vue 的 animate-apple-fade-in 动画(fill both)会永久留下
         translateY(0) 变换，任何非 none 的 transform 都会把后代 fixed 定位锚到该祖先，
         传送出层后 fixed 才相对视口 -->
    <Teleport to="body">
      <div
        v-if="ctxMenu && ctxRow"
        class="ctx-overlay"
        @click="closeCtxMenu"
        @contextmenu.prevent="closeCtxMenu"
      >
      <div
        ref="ctxRef"
        class="ctx-menu"
        :style="{ left: `${ctxX}px`, top: `${ctxY}px` }"
        @click.stop
        @contextmenu.stop.prevent
      >
        <button type="button" class="ctx-item" :disabled="ctxRow.testing" @click="onPickUsage">
          <v-icon icon="mdi-chart-areaspline" size="16" />
          <span>{{ usageKeyId === ctxRow.id ? "✓ 正在查看该 Key" : "查看该 Key 额度" }}</span>
        </button>
        <div class="ctx-sep" />
        <!-- 模型二级子菜单：悬停（约 220ms 意图延迟）或点击展开，与托盘级联子菜单同款 -->
        <button
          type="button"
          class="ctx-item ctx-item--parent"
          :class="{ 'ctx-item--open': ctxSubOpen }"
          :disabled="ctxRow.testing"
          @mouseenter="ctxSubHoverOpen"
          @mouseleave="ctxSubHoverClose"
          @click="ctxSubClick"
        >
          <v-icon icon="mdi-file-tree-outline" size="16" />
          <span>选择模型测试</span>
          <v-icon icon="mdi-chevron-right" size="14" class="ctx-item-arrow" />
        </button>
        <div class="ctx-sep" />
        <button type="button" class="ctx-item" :disabled="ctxRow.testing" @click="ctxToggle">
          <v-icon :icon="ctxRow.enabled ? 'mdi-circle-outline' : 'mdi-circle'" size="16" />
          <span>{{ ctxRow.enabled ? "禁用该 Key" : "启用该 Key" }}</span>
        </button>
      </div>

      <!-- 二级子菜单：固定定位跟随触发项，贴边自动翻转；点击模型即测 -->
      <div
        v-if="ctxSubOpen"
        ref="ctxSubRef"
        class="ctx-sub"
        :style="{ left: `${ctxSubX}px`, top: `${ctxSubY}px`, maxHeight: `${ctxSubMaxH}px` }"
        @mouseenter="ctxSubHoverStay"
        @mouseleave="ctxSubHoverClose"
        @click.stop
        @contextmenu.stop.prevent
      >
        <div v-if="ctxModelsLoading" class="ctx-sub-hint">
          <v-progress-circular indeterminate size="14" width="2" />
          正在获取模型列表…
        </div>
        <div v-else-if="!ctxModels.length" class="ctx-sub-hint">该 Key 没有可用的模型列表</div>
        <template v-else>
          <button
            v-for="m in ctxModels"
            :key="m.id"
            type="button"
            class="ctx-sub-item"
            :title="m.display_name || m.id"
            @click="ctxTestWith(m)"
          >
            <span class="ctx-sub-name">{{ m.display_name || m.id }}</span>
          </button>
        </template>
      </div>
      </div>
    </Teleport>
  </div>
</template>


