<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  getAccountModels,
  handleErr,
  refreshAccounts,
  refreshGroups,
  selectGroup,
  setSchedulable,
  store,
  testAccount,
  testAll,
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

const headers = [
  { title: "账号", key: "name", sortable: true, width: 190 },
  { title: "状态", key: "status", sortable: true, width: 80 },
  { title: "调度", key: "schedulable", sortable: true, width: 130 },
  { title: "首 token", key: "first_token", sortable: true, width: 96 },
  { title: "总耗时", key: "total", sortable: true, width: 84 },
  { title: "备注 / 最近错误", key: "note", sortable: false },
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
}

const rows = computed<Row[]>(() =>
  store.accounts.map((a) => {
    const r = store.results[a.id];
    return {
      ...a,
      result: r,
      testing: store.testingIds.has(a.id),
      // 排序键：无数据用极大值，保证升序时已测账号在前
      first_token: r?.first_token_ms ?? Number.MAX_SAFE_INTEGER,
      total: r?.total_ms ?? Number.MAX_SAFE_INTEGER,
    };
  }),
);

function firstTokenColor(ms: number): string {
  if (ms < 2000) return "success";
  if (ms < 5000) return "warning";
  return "error";
}

/// 行背景按状态着色：错误 > 限流/临时 > 启用中 > 默认；右键行弹出快捷菜单
function rowProps({ item }: { item: Row }): Record<string, unknown> {
  const cls =
    item.status === "error"
      ? "s2a-row-error"
      : item.rate_limited || item.temp_unschedulable
        ? "s2a-row-warn"
        : item.schedulable
          ? "s2a-row-active"
          : "";
  return { class: cls, onContextmenu: (e: MouseEvent) => openCtxMenu(e, item) };
}

function firstTokenClass(r: TestResult): string {
  if (!r.success) return "text-error";
  if (r.first_token_ms == null) return "text-medium-emphasis";
  return `text-${firstTokenColor(r.first_token_ms)}`;
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
}

function ctxTest() {
  const row = ctxRow.value;
  closeCtxMenu();
  if (row && !row.testing) void testAccount(row.id);
}

function ctxOpenModels() {
  const row = ctxRow.value;
  closeCtxMenu();
  if (row && !row.testing) void openModelDialog(row);
}

function ctxToggle() {
  const row = ctxRow.value;
  closeCtxMenu();
  if (row && !row.testing) void setSchedulable(row.id, !row.schedulable);
}

function onCtxKey(e: KeyboardEvent) {
  if (e.key === "Escape" && ctxMenu.value) closeCtxMenu();
}

onMounted(() => window.addEventListener("keydown", onCtxKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onCtxKey));
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

    <!-- 工具栏传送到顶部应用栏（App.vue 的 .appbar-tools）：与品牌、导航、登录状态合并为一行。
         分组选择做成 macOS 工具栏弹出菜单风格的胶囊，紧凑安静。
         v-window 非活跃项仅隐藏不卸载，常驻 Teleport 会在设置页泄漏，故仅账号页挂载 -->
    <Teleport v-else-if="store.tab === 'accounts'" defer to=".appbar-tools">
      <div class="d-flex align-center ga-2" style="min-width: 0">
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
              <v-list-item-title class="text-body-2">{{ g.title }}</v-list-item-title>
            </v-list-item>
          </v-list>
        </v-menu>
        <v-tooltip text="刷新分组与账号">
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
        <v-btn
          color="primary"
          prepend-icon="mdi-speedometer"
          :loading="store.testingAll"
          :disabled="!store.accounts.length"
          @click="testAll"
        >
          测试全部
        </v-btn>
      </div>
    </Teleport>

    <div v-if="store.auth" class="table-card">
      <v-data-table
        :headers="headers"
        :items="rows"
        :loading="store.loadingAccounts"
        :row-props="rowProps"
        density="compact"
        items-per-page="50"
        hover
        no-data-text="暂无账号（请选择分组）"
      >
        <template #item.name="{ item }">
          <v-tooltip :disabled="!item.result" max-width="420">
            <template #activator="{ props: tipProps }">
              <div v-bind="tipProps" style="min-width: 0">
                <div class="text-body-2 font-weight-medium text-truncate">{{ item.name }}</div>
                <div class="text-caption text-disabled text-truncate">
                  #{{ item.id }} · {{ item.platform }}/{{ item.type }}
                </div>
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
          <v-chip
            size="x-small"
            :color="item.status === 'active' ? 'success' : item.status === 'error' ? 'error' : 'grey'"
            variant="tonal"
          >
            {{ statusText(item.status) }}
          </v-chip>
        </template>

        <template #item.schedulable="{ item }">
          <v-chip size="x-small" :color="item.schedulable ? 'primary' : 'default'" variant="tonal">
            {{ item.schedulable ? "启用" : "禁用" }}
          </v-chip>
          <v-chip v-if="item.rate_limited" size="x-small" color="warning" variant="tonal" class="ml-1">限流</v-chip>
          <v-chip v-if="item.temp_unschedulable" size="x-small" color="warning" variant="tonal" class="ml-1">临时</v-chip>
        </template>

        <template #item.first_token="{ item }">
          <v-progress-circular v-if="item.testing" indeterminate size="14" width="2" />
          <template v-else-if="item.result">
            <span v-if="item.result.first_token_ms != null" :class="firstTokenClass(item.result)" class="text-body-2 font-weight-medium">
              {{ item.result.first_token_ms }} ms
            </span>
            <v-icon v-else-if="!item.result.success" color="error" size="small" icon="mdi-close-circle" />
            <span v-else class="text-medium-emphasis">—</span>
          </template>
          <span v-else class="text-medium-emphasis">—</span>
        </template>

        <template #item.total="{ item }">
          <span v-if="item.result?.total_ms != null" class="text-body-2">{{ item.result.total_ms }} ms</span>
          <span v-else class="text-medium-emphasis">—</span>
        </template>

        <template #item.note="{ item }">
          <v-tooltip v-if="noteText(item)" :text="noteText(item)" location="top">
            <template #activator="{ props }">
              <span
                v-bind="props"
                class="d-inline-block text-truncate text-caption text-error"
                style="max-width: 100%"
              >
                {{ noteText(item) }}
              </span>
            </template>
          </v-tooltip>
          <v-tooltip
            v-else-if="item.result?.success"
            :text="resultTooltip(item.result)"
            location="top"
          >
            <template #activator="{ props }">
              <span v-bind="props" class="text-caption text-success">✓ 正常</span>
            </template>
          </v-tooltip>
          <span v-else class="text-medium-emphasis">—</span>
        </template>

        <template #item.actions="{ item }">
          <v-btn
            size="small"
            variant="text"
            color="primary"
            :loading="item.testing"
            @click="testAccount(item.id)"
          >
            测试
          </v-btn>
          <v-tooltip text="选择模型测试">
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
          <v-btn
            size="small"
            variant="text"
            class="ml-1"
            :disabled="item.testing"
            @click="setSchedulable(item.id, !item.schedulable)"
          >
            {{ item.schedulable ? "禁用" : "启用" }}
          </v-btn>
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

    <!-- 行右键快捷菜单：跟随光标，点消（与托盘菜单一致，右键空白处收起） -->
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
        <div class="ctx-title" :title="ctxRow.name">{{ ctxRow.name }}</div>
        <button type="button" class="ctx-item" :disabled="ctxRow.testing" @click="ctxTest">
          <v-icon icon="mdi-speedometer" size="16" />
          <span>测试该账号</span>
        </button>
        <button type="button" class="ctx-item" :disabled="ctxRow.testing" @click="ctxOpenModels">
          <v-icon icon="mdi-file-tree-outline" size="16" />
          <span>选择模型测试…</span>
        </button>
        <div class="ctx-sep" />
        <button type="button" class="ctx-item" :disabled="ctxRow.testing" @click="ctxToggle">
          <v-icon :icon="ctxRow.schedulable ? 'mdi-circle-outline' : 'mdi-circle'" size="16" />
          <span>{{ ctxRow.schedulable ? "禁用该账号" : "启用该账号" }}</span>
        </button>
      </div>
    </div>
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

/* 表格卡片容器：白底 + 发丝边框 + 12px 圆角 */
.table-card {
  background: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: 12px;
  overflow: hidden;
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.04);
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
  font-weight: 600 !important;
  color: #6e6e73 !important;
  background: #fafafb;
}
/* 行分隔发丝线 + 悬浮浅灰 */
:deep(.v-data-table__td) {
  border-bottom: 1px solid rgba(0, 0, 0, 0.05);
}
:deep(.v-data-table__tr:hover > .v-data-table__td) {
  background: rgba(0, 0, 0, 0.03) !important;
}
/* 行背景按状态着色（Apple 语义色低透明度） */
:deep(.s2a-row-error) {
  background: rgba(255, 59, 48, 0.05);
}
:deep(.s2a-row-warn) {
  background: rgba(255, 149, 0, 0.06);
}
:deep(.s2a-row-active) {
  background: rgba(52, 199, 89, 0.05);
}
:deep(tr.s2a-row-error:hover) td,
:deep(.v-data-table__tr.s2a-row-error:hover) td {
  background: rgba(255, 59, 48, 0.1) !important;
}
:deep(tr.s2a-row-warn:hover) td,
:deep(.v-data-table__tr.s2a-row-warn:hover) td {
  background: rgba(255, 149, 0, 0.12) !important;
}
:deep(tr.s2a-row-active:hover) td,
:deep(.v-data-table__tr.s2a-row-active:hover) td {
  background: rgba(52, 199, 89, 0.1) !important;
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
.ctx-title {
  padding: 6px 10px 5px;
  font-size: 12px;
  font-weight: 600;
  color: hsl(var(--muted-foreground));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: none;
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
</style>
