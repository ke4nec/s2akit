<script setup lang="ts">
import { computed, h, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  getAccountModels,
  handleErr,
  refreshAccounts,
  refreshGroups,
  selectGroup,
  setSchedulable,
  store,
  testAccount,
} from "../store";
import type { ModelBrief, TestResult } from "../types";

const groupItems = computed(() =>
  store.groups.map((g) => ({
    title: `${g.name} · ${g.platform}（${g.account_count ?? "?"}${g.status !== "active" ? "，停用" : ""}）`,
    value: g.id,
  })),
);

const selectedGroup = computed({
  get: () => store.config?.group_id ?? null,
  set: (v: number | null) => {
    if (v != null) void selectGroup(v);
  },
});

/** 顶栏分组胶囊上显示的当前分组名 */
const currentGroupTitle = computed(
  () => groupItems.value.find((g) => g.value === selectedGroup.value)?.title ?? "选择分组",
);

/// 表头排序指示（Apple 风，对齐 design/accounts.html）：
/// 标题 + 成对 8×5px 小 chevron；当前排序列标题加深、方向对应的一枚点亮（上＝升序、下＝降序）
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

/// 状态列排序权重：正常 → 停用 → 错误（升降序沿此语义）
function statusRank(s: string): number {
  if (s === "active") return 0;
  if (s === "inactive") return 1;
  return 2;
}

const headers = [
  { title: "账号", key: "name", sortable: true, width: 216 },
  {
    title: "状态",
    key: "status",
    sortable: true,
    width: 84,
    // Vuetify 表头 sort 收到的是列的值（status 字符串），按语义权重排序
    sort: (a: string, b: string) => statusRank(a) - statusRank(b),
  },
  { title: "调度", key: "schedulable", sortable: true, width: 132 },
  { title: "首 token", key: "first_token", sortable: true, width: 104 },
  { title: "总耗时", key: "total", sortable: true, width: 96 },
  {
    title: "备注 / 最近错误",
    key: "note",
    sortable: true,
    // 排序键为 rows 预计算的 0/1（无错误在前、有错误在后）
    sort: (a: number, b: number) => a - b,
  },
  { title: "操作", key: "actions", sortable: false, align: "end" as const, width: 150 },
];

interface Row {
  id: number;
  name: string;
  platform: string;
  type: string;
  status: string;
  schedulable: boolean;
  rate_limited: boolean;
  temp_unschedulable: boolean;
  error_message: string;
  result?: TestResult;
  testing: boolean;
  first_token: number;
  total: number;
  /** 备注列排序键：1＝有错误信息（测试错误或账号错误），0＝无 */
  note: number;
}

const rows = computed<Row[]>(() =>
  store.accounts.map((a) => {
    const r = store.results[a.id];
    const row: Row = {
      ...a,
      result: r,
      testing: store.testingIds.has(a.id),
      // 排序键：无数据用极大值，保证升序时已测账号在前
      first_token: r?.first_token_ms ?? Number.MAX_SAFE_INTEGER,
      total: r?.total_ms ?? Number.MAX_SAFE_INTEGER,
      note: 0,
    };
    row.note = noteText(row) ? 1 : 0;
    return row;
  }),
);

/// 分页状态：每页条数（默认 10，与窗口默认高度匹配）；页码由表格内部管理，经 #bottom 插槽交互
const itemsPerPage = ref(10);
const itemsPerPageOptions = [10, 20, 50, 100];

/// 页码窗口：页数不多时全量展示，页数多时收敛为 首页/尾页/当前±1，0 代表省略号
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

/// 行状态判定：错误 > 限流/临时 > 正常 > 停用，驱动行首竖条与汇总计数
function rowState(item: Row): "ok" | "warn" | "error" | "off" {
  if (item.status === "error") return "error";
  if (item.rate_limited || item.temp_unschedulable) return "warn";
  if (item.schedulable && item.status === "active") return "ok";
  return "off";
}

/// 卡片头状态汇总：替代原整行底色的全局概览
const stateCounts = computed(() => {
  const c = { ok: 0, warn: 0, error: 0, off: 0 };
  for (const r of rows.value) c[rowState(r)] += 1;
  return c;
});

/// 右键行弹出快捷菜单
function rowProps({ item }: { item: Row }): Record<string, unknown> {
  return { onContextmenu: (e: MouseEvent) => openCtxMenu(e, item) };
}

/// 状态徽标配色：正常绿 / 错误红 / 其余灰
function statusBadgeClass(s: string): string {
  if (s === "active") return "s2a-tag--ok";
  if (s === "error") return "s2a-tag--error";
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

function noteText(row: Row): string {
  const parts = [row.result?.error, row.error_message].filter(
    (s): s is string => !!s && s.trim().length > 0,
  );
  return parts.join("｜");
}

function resultTooltip(r: TestResult): string {
  const model = r.model || "默认模型";
  const reply = r.content_preview || "（空）";
  return `模型：${model}\n回复：${reply}`;
}

async function onRefresh() {
  await refreshGroups();
  await refreshAccounts();
}

function goSettings() {
  store.tab = "settings";
}

// ---- 单账号模型选择测试 ----
const modelDialog = ref(false);
const modelAccount = ref<Row | null>(null);
const models = ref<ModelBrief[]>([]);
const loadingModels = ref(false);

async function openModelDialog(row: Row) {
  modelAccount.value = row;
  models.value = [];
  modelDialog.value = true;
  loadingModels.value = true;
  try {
    models.value = await getAccountModels(row.id);
  } catch (e) {
    modelDialog.value = false;
    handleErr(e);
  } finally {
    loadingModels.value = false;
  }
}

function testWithModel(model: ModelBrief) {
  if (!modelAccount.value) return;
  modelDialog.value = false;
  void testAccount(modelAccount.value.id, model.id);
}

// ---- 行右键快捷菜单：测试 / 选择模型测试 / 启用禁用 ----
const ctxMenu = ref(false);
const ctxRow = ref<Row | null>(null);
const ctxX = ref(0);
const ctxY = ref(0);
const ctxRef = ref<HTMLElement | null>(null);

function openCtxMenu(e: MouseEvent, row: Row) {
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
  if (row && !row.testing) void setSchedulable(row.id, !row.schedulable);
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
  // 模型列表按账号懒加载，列表为空时才请求（关闭菜单时清空）
  if (!ctxModels.value.length && !ctxModelsLoading.value) void loadCtxModels();
}

async function loadCtxModels() {
  const row = ctxRow.value;
  if (!row) return;
  ctxModelsLoading.value = true;
  try {
    ctxModels.value = await getAccountModels(row.id);
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
  if (row && !row.testing) void testAccount(row.id, m.id);
}

function onCtxKey(e: KeyboardEvent) {
  if (e.key === "Escape" && ctxMenu.value) closeCtxMenu();
}

onMounted(() => window.addEventListener("keydown", onCtxKey));
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
      <!-- 状态汇总：左侧状态计数，右侧分组选择 + 刷新 -->
      <div class="s2a-summary">
        <span class="s2a-sum">
          <i class="s2a-sum-dot s2a-dot--ok" aria-hidden="true"></i>正常 <b>{{ stateCounts.ok }}</b>
        </span>
        <span class="s2a-sum">
          <i class="s2a-sum-dot s2a-dot--warn" aria-hidden="true"></i>限流 / 临时 <b>{{ stateCounts.warn }}</b>
        </span>
        <span class="s2a-sum">
          <i class="s2a-sum-dot s2a-dot--error" aria-hidden="true"></i>错误 <b>{{ stateCounts.error }}</b>
        </span>
        <span class="s2a-sum">
          <i class="s2a-sum-dot s2a-dot--off" aria-hidden="true"></i>停用 <b>{{ stateCounts.off }}</b>
        </span>
        <div class="s2a-summary-tools">
          <v-menu scroll-strategy="close">
            <template #activator="{ props: menuProps }">
              <button v-bind="menuProps" type="button" class="group-pop">
                <span class="group-pop-text" :title="currentGroupTitle">{{ currentGroupTitle }}</span>
                <v-progress-circular
                  v-if="store.loadingGroups"
                  indeterminate
                  size="12"
                  width="1.5"
                  class="group-pop-wait"
                />
                <v-icon v-else icon="mdi-chevron-down" size="15" class="group-pop-chevron" />
              </button>
            </template>
            <v-list density="compact" class="group-list" max-height="360">
              <v-list-item v-for="g in groupItems" :key="g.value" @click="selectedGroup = g.value">
                <template #prepend>
                  <v-icon
                    :icon="g.value === selectedGroup ? 'mdi-check' : 'mdi-circle-medium'"
                    :color="g.value === selectedGroup ? 'primary' : 'grey'"
                    size="15"
                  />
                </template>
                <v-list-item-title>{{ g.title }}</v-list-item-title>
              </v-list-item>
            </v-list>
          </v-menu>
          <v-tooltip text="刷新分组与账号" content-class="apple-tip">
            <template #activator="{ props }">
              <v-btn
                v-bind="props"
                icon="mdi-refresh"
                variant="text"
                size="small"
                :loading="store.loadingAccounts"
                @click="onRefresh"
              />
            </template>
          </v-tooltip>
        </div>
      </div>
      <v-data-table
        :headers="headers"
        :items="rows"
        :loading="store.loadingAccounts"
        :row-props="rowProps"
        v-model:items-per-page="itemsPerPage"
        density="compact"
        hover
        no-data-text="暂无账号（请选择分组）"
      >
        <!-- 表头：Apple 风成对 chevron 排序指示（自定义插槽替换默认图标，th 点击排序仍由 Vuetify 处理）；
             函数组件未声明 props，绑定需用 camelCase :sortBy，kebab 不会归一化 -->
        <template #header.name="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.status="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.schedulable="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.first_token="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.total="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>
        <template #header.note="{ column, sortBy }"><HeaderCell :column="column" :sortBy="sortBy" /></template>

        <template #item.name="{ item }">
          <v-tooltip :disabled="!item.result" max-width="420" content-class="apple-tip">
            <template #activator="{ props: tipProps }">
              <div v-bind="tipProps" style="min-width: 0">
                <span class="s2a-bar" :class="`s2a-bar--${rowState(item)}`" aria-hidden="true"></span>
                <div class="s2a-name">{{ item.name }}</div>
                <div class="s2a-meta">#{{ item.id }} · {{ item.platform }}/{{ item.type }}</div>
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

        <template #item.schedulable="{ item }">
          <div class="s2a-sw-wrap">
            <button
              type="button"
              class="s2a-sw"
              :class="{ 's2a-sw--on': item.schedulable }"
              role="switch"
              :aria-checked="item.schedulable"
              :aria-label="item.schedulable ? '禁用该账号' : '启用该账号'"
              :disabled="item.testing"
              @click="setSchedulable(item.id, !item.schedulable)"
            ></button>
            <span v-if="item.rate_limited" class="s2a-tag s2a-tag--warn s2a-tag--sm">限流</span>
            <span v-if="item.temp_unschedulable" class="s2a-tag s2a-tag--warn s2a-tag--sm">临时</span>
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

        <template #item.note="{ item }">
          <v-tooltip v-if="noteText(item)" :text="noteText(item)" location="top" content-class="apple-tip">
            <template #activator="{ props }">
              <span v-bind="props" class="s2a-note s2a-note--err">
                {{ noteText(item) }}
              </span>
            </template>
          </v-tooltip>
          <v-tooltip
            v-else-if="item.result?.success"
            :text="resultTooltip(item.result)"
            location="top"
            content-class="apple-tip"
          >
            <template #activator="{ props }">
              <span v-bind="props" class="s2a-note s2a-note--ok">✓ 正常</span>
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
            @click="testAccount(item.id)"
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

        <!-- 卡片底分页条：替换默认 footer，样式对齐 design/accounts.html -->
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
            <span class="s2a-pager-total">共 <b>{{ itemsLength }}</b> 个账号</span>
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
                  class="s2a-pg-btn"
                  type="button"
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
          <div v-if="modelAccount" class="model-dialog-sub">{{ modelAccount.name }}</div>
        </div>
        <v-progress-linear v-if="loadingModels" indeterminate color="primary" />
        <div class="model-dialog-body">
          <div v-if="!loadingModels && !models.length" class="text-body-2 text-medium-emphasis">
            该账号没有可用的模型列表
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
         translateY(0) 变换，任何非 none 的 transform 都会把后代 fixed 定位锚到该祖先
         （实测菜单整体偏移 +24/+72），传送出层后 fixed 才相对视口 -->
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
          <v-icon :icon="ctxRow.schedulable ? 'mdi-circle-outline' : 'mdi-circle'" size="16" />
          <span>{{ ctxRow.schedulable ? "禁用该账号" : "启用该账号" }}</span>
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
        <div v-else-if="!ctxModels.length" class="ctx-sub-hint">该账号没有可用的模型列表</div>
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

<style scoped>
/* 分组选择胶囊：macOS 工具栏弹出菜单风格，28px 高、灰底圆角、悬浮加深；
   与顶栏分段导航（同色系容器）视觉统一 */
.group-pop {
  flex: none;
  height: 28px;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  min-width: 0;
  padding: 0 7px 0 11px;
  border: none;
  border-radius: 7px;
  background: rgba(118, 118, 128, 0.12);
  font-family: inherit;
  font-size: 12px;
  font-weight: 500;
  letter-spacing: -0.01em;
  color: rgba(0, 0, 0, 0.78);
  cursor: pointer;
  user-select: none;
  transition: background 0.15s var(--ease-in-out, ease);
}
.group-pop:hover {
  background: rgba(118, 118, 128, 0.2);
}
.group-pop:focus-visible {
  outline: 2px solid rgba(0, 122, 255, 0.45);
}
.group-pop-text {
  min-width: 0;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.group-pop-chevron,
.group-pop-wait {
  color: rgba(0, 0, 0, 0.4);
}
/* 分组下拉列表：限高滚动，其余沿用 Vuetify 菜单默认质感 */
.group-list {
  max-height: 360px;
  overflow-y: auto;
}

/* 表格卡片容器：白底 + 发丝边框 + 8px 圆角（对齐 design/accounts.html） */
.table-card {
  background: hsl(var(--background));
  border: 1px solid #ebeef5;
  border-radius: 8px;
  overflow: hidden;
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.03);
}

/* 卡片头状态汇总：左侧状态计数，右侧分组选择 + 刷新 */
.s2a-summary {
  display: flex;
  align-items: center;
  gap: 18px;
  min-height: 44px;
  padding: 6px 20px;
  background: #fafbfc;
  border-bottom: 1px solid #ebeef5;
}
.s2a-summary-tools {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.s2a-sum {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: #606266;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.s2a-sum b {
  font-weight: 600;
  color: #303133;
}
.s2a-sum-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex: none;
}
.s2a-dot--ok { background: #34c759; }
.s2a-dot--warn { background: #ff9500; }
.s2a-dot--error { background: #ff3b30; }
.s2a-dot--off { background: #c7c7cc; }
/* 行首状态竖条：Apple 系统色，替代原整行底色；绝对定位于行首（td 为定位锚） */
.s2a-bar {
  position: absolute;
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 22px;
  border-radius: 0 2px 2px 0;
}
.s2a-bar--ok { background: #34c759; }
.s2a-bar--warn { background: #ff9500; }
.s2a-bar--error { background: #ff3b30; }
.s2a-bar--off { background: #c7c7cc; }

/* 状态徽标：4px 小圆角、12px/500 字号，浅底深字（文本用 HIG 加深色保证小字对比度） */
.s2a-tag {
  display: inline-flex;
  align-items: center;
  height: 22px;
  padding: 0 8px;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
  line-height: 1;
  white-space: nowrap;
}
.s2a-tag--sm { height: 19px; padding: 0 6px; }
.s2a-tag--ok { background: rgba(52, 199, 89, 0.13); color: #248a3d; }
.s2a-tag--warn { background: rgba(255, 149, 0, 0.14); color: #c55300; }
.s2a-tag--error { background: rgba(255, 59, 48, 0.12); color: #ff3b30; }
.s2a-tag--off { background: rgba(142, 142, 147, 0.14); color: #6e6e73; }

/* 账号列：名称 + 元信息两行 */
.s2a-name {
  font-size: 14px;
  font-weight: 600;
  color: #303133;
  line-height: 1.3;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.s2a-meta {
  margin-top: 2px;
  font-size: 12px;
  color: #909399;
  line-height: 1.3;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 调度列：iOS 风迷你开关（32×20），开＝Apple 绿、关＝系统灰轨；限流/临时小徽标跟在开关后 */
.s2a-sw-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
}
.s2a-sw {
  position: relative;
  flex: none;
  width: 32px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 10px;
  background: rgba(120, 120, 128, 0.32);
  cursor: pointer;
  transition: background 0.2s var(--ease-in-out, ease);
}
.s2a-sw::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  transition: transform 0.2s var(--ease-in-out, ease);
}
.s2a-sw:hover:not(:disabled) {
  background: rgba(120, 120, 128, 0.44);
}
.s2a-sw--on {
  background: #34c759;
}
.s2a-sw--on::after {
  transform: translateX(12px);
}
.s2a-sw--on:hover:not(:disabled) {
  background: #2db94e;
}
.s2a-sw:disabled {
  opacity: 0.45;
  cursor: default;
}
.s2a-sw:focus-visible {
  outline: 2px solid rgba(0, 122, 255, 0.45);
  outline-offset: 1px;
}

/* 数字列：13px 半粗 + 等宽数字，按阈值着色；测试中蓝色转圈 */
.s2a-num {
  font-size: 13px;
  font-weight: 600;
  color: #303133;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.s2a-unit {
  font-size: 11px;
  font-weight: 400;
  color: #909399;
  margin-left: 1px;
}
.s2a-num--ok { color: #248a3d; }
.s2a-num--mid { color: #c55300; }
.s2a-num--bad { color: #ff3b30; }
.s2a-none {
  font-size: 13px;
  color: #c7c7cc;
}
/* 测试中骨架：灰色横线 shimmer 扫光（无旋转），首 token 与总耗时两列同款 */
.s2a-skeleton {
  display: inline-block;
  width: 56px;
  height: 10px;
  border-radius: 5px;
  background: linear-gradient(
    90deg,
    rgba(0, 0, 0, 0.06) 25%,
    rgba(0, 0, 0, 0.12) 37%,
    rgba(0, 0, 0, 0.06) 63%
  );
  background-size: 400% 100%;
  animation: s2a-shimmer 1.4s ease infinite;
}
@keyframes s2a-shimmer {
  0% {
    background-position: 100% 50%;
  }
  100% {
    background-position: 0 50%;
  }
}
@media (prefers-reduced-motion: reduce) {
  .s2a-skeleton {
    animation: none;
  }
}

/* 备注列 */
.s2a-note {
  display: inline-block;
  max-width: 100%;
  font-size: 12px;
  line-height: 1.35;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: middle;
}
.s2a-note--ok { color: #248a3d; }
.s2a-note--err { color: #ff3b30; }

/* 卡片底分页条：与卡片头汇总条同底色上下呼应；每页选择器 + 页码按钮（当前页蓝底白字） */
.s2a-pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-height: 46px;
  padding: 6px 20px;
  background: #fafbfc;
  border-top: 1px solid #ebeef5;
}
.s2a-pager-total {
  font-size: 12px;
  color: #909399;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.s2a-pager-total b {
  font-weight: 600;
  color: #303133;
}
.s2a-pager-right {
  display: flex;
  align-items: center;
  gap: 14px;
}
.s2a-pager-size {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: #909399;
  white-space: nowrap;
}
.s2a-pager-select {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 8px 0 10px;
  border: none;
  border-radius: 6px;
  background: #f5f7fa;
  color: #303133;
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
}
.s2a-pager-select:hover {
  background: #eef1f6;
}
.s2a-pager-nav {
  display: flex;
  align-items: center;
  gap: 4px;
}
.s2a-pg-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 28px;
  padding: 0 6px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: #606266;
  font-family: inherit;
  font-size: 12.5px;
  font-variant-numeric: tabular-nums;
  cursor: pointer;
}
.s2a-pg-btn:hover:not(:disabled):not(.s2a-pg-cur) {
  background: #f5f7fa;
  color: #303133;
}
.s2a-pg-btn:disabled {
  opacity: 0.35;
  cursor: default;
}
.s2a-pg-cur {
  background: #007aff;
  color: #fff;
  font-weight: 600;
}
.s2a-pg-ellipsis {
  align-self: flex-end;
  padding: 0 2px 4px;
  font-size: 12px;
  color: #909399;
}
/* 固定表格布局：列宽严格按 header 设定分配，备注列吸收剩余空间。
   自动布局下备注列的不换行长文本会把整张表撑出容器、挤走操作列 */
:deep(.v-data-table table) {
  table-layout: fixed;
}
/* 表头禁止折行，避免窄列标题被挤成两行 */
:deep(.v-data-table__th) {
  white-space: nowrap;
  font-size: 12px !important;
  font-weight: 500 !important;
  color: #909399 !important;
}
/* 行分隔发丝线 + 悬浮浅灰；td 作为行首竖条的定位锚 */
:deep(.v-data-table__td) {
  position: relative;
  border-bottom: 1px solid #f2f3f5;
}
:deep(.v-data-table__td:first-child) {
  padding-left: 20px;
}
/* 表头首/末列同步内边距，与内容列对齐 */
:deep(.v-data-table__th:first-child) {
  padding-left: 20px;
}
:deep(.v-data-table__th:last-child),
:deep(.v-data-table__td:last-child) {
  padding-right: 20px;
}
:deep(.v-data-table__tr:hover > .v-data-table__td) {
  background: #f5f7fa !important;
}
:deep(.v-data-table__tr:last-child .v-data-table__td) {
  border-bottom: none;
}
/* 可排序列头悬停文字加深；排序指示样式见文件末尾非 scoped 块（h() 渲染的元素不带 data-v） */
:deep(.v-data-table__th--sortable:hover) {
  color: #606266 !important;
}

/* 模型选择对话框 */
.model-dialog {
  padding: 20px 20px 16px;
}
.model-dialog-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: hsl(var(--foreground));
}
.model-dialog-sub {
  margin-top: 2px;
  font-size: 13px;
  color: hsl(var(--muted-foreground));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.model-dialog-body {
  margin-top: 12px;
  min-height: 48px;
}
.model-list {
  border: 1px solid hsl(var(--border));
  border-radius: 10px;
  overflow: hidden;
}
.model-dialog-foot {
  display: flex;
  justify-content: flex-end;
  margin-top: 12px;
}

/* 行右键快捷菜单：透明全屏点消层 + 跟随光标的浮层卡片 */
.ctx-overlay {
  position: fixed;
  inset: 0;
  z-index: 2500;
  background: transparent;
}
.ctx-menu {
  position: fixed;
  width: 184px;
  padding: 4px;
  background: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: 10px;
  box-shadow:
    0 8px 28px rgb(0 0 0 / 0.14),
    0 0 0 0.5px rgb(0 0 0 / 0.02);
}
.ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  font-family: inherit;
  font-size: 13px;
  color: hsl(var(--foreground));
  cursor: pointer;
  text-align: left;
}
.ctx-item:hover:not(:disabled) {
  background: hsl(var(--muted));
}
.ctx-item:disabled {
  opacity: 0.45;
  cursor: default;
}
.ctx-sep {
  height: 1px;
  margin: 4px 6px;
  background: hsl(var(--border));
}

/* 模型二级子菜单：与主菜单同质感，固定定位跟随触发项；
   最大高度由脚本按视口钳制（ctxSubMaxH，320px 封顶），底部时整体往上长 */
.ctx-sub {
  position: fixed;
  z-index: 2501;
  width: 220px;
  overflow-y: auto;
  padding: 4px;
  background: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: 10px;
  box-shadow:
    0 8px 28px rgb(0 0 0 / 0.14),
    0 0 0 0.5px rgb(0 0 0 / 0.02);
}
.ctx-sub-item {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 7px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  font-family: inherit;
  font-size: 13px;
  color: hsl(var(--foreground));
  cursor: pointer;
  text-align: left;
}
.ctx-sub-item:hover {
  background: hsl(var(--muted));
}
/* 模型名超长截断，悬停靠 title 提示显示全名（同托盘子菜单） */
.ctx-sub-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ctx-sub-hint {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  font-size: 12px;
  color: hsl(var(--muted-foreground));
}
.ctx-item--open {
  background: hsl(var(--muted));
}
.ctx-item-arrow {
  margin-left: auto;
  color: hsl(var(--muted-foreground));
}
</style>

<style>
/* 表头排序指示（Apple 风成对 chevron）：HeaderCell 为函数组件，元素经 h() 渲染、
   不带 scoped 的 data-v 属性，样式须放非 scoped 块才能命中；
   类名沿用 s2a- 前缀全局命名约定（同 apple.css 的 .s2a-flag） */
.s2a-th--active {
  color: #606266;
}
.s2a-si {
  display: inline-flex;
  flex-direction: column;
  gap: 1px;
  margin-left: 4px;
  vertical-align: middle;
}
.s2a-si svg {
  display: block;
  width: 8px;
  height: 5px;
  color: #c7c7cc;
}
.s2a-si svg.s2a-si-on {
  color: #6e6e73;
}
</style>
