<script setup lang="ts">
import { computed, ref } from "vue";
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

/// 行背景按状态着色：错误 > 限流/临时 > 启用中 > 默认
function rowProps({ item }: { item: Row }): Record<string, unknown> {
  if (item.status === "error") return { class: "s2a-row-error" };
  if (item.rate_limited || item.temp_unschedulable) return { class: "s2a-row-warn" };
  if (item.schedulable) return { class: "s2a-row-active" };
  return {};
}

function firstTokenClass(r: TestResult): string {
  if (!r.success) return "text-error";
  if (r.first_token_ms == null) return "text-medium-emphasis";
  return `text-${firstTokenColor(r.first_token_ms)}`;
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
</script>

<template>
  <div>
    <v-alert
      v-if="!store.auth"
      type="info"
      variant="tonal"
      density="compact"
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

    <div v-else class="mb-3">
      <div class="d-flex align-center ga-2">
        <v-select
          v-model="selectedGroup"
          :items="groupItems"
          label="分组"
          density="compact"
          hide-details
          style="max-width: 340px; min-width: 220px"
          :loading="store.loadingGroups"
          placeholder="选择分组"
        />
        <v-tooltip text="刷新分组与账号">
          <template #activator="{ props }">
            <v-btn
              v-bind="props"
              icon="mdi-refresh"
              size="small"
              variant="text"
              :loading="store.loadingAccounts"
              @click="onRefresh"
            />
          </template>
        </v-tooltip>
        <v-spacer />
        <v-btn
          size="small"
          color="primary"
          prepend-icon="mdi-speedometer"
          :loading="store.testingAll"
          :disabled="!store.accounts.length"
          @click="testAll"
        >
          测试全部
        </v-btn>
      </div>
      <div class="text-caption text-disabled text-truncate mt-1">
        {{
          store.config?.default_model?.trim() ||
          "自动（上次测试模型 → 平台默认：OpenAI 用 astra、Claude 用 opus → 列表首个文本模型）"
        }}
        · Prompt：{{ store.config?.test_prompt }} · 并发：{{ store.config?.test_concurrency }}
      </div>
    </div>

    <v-data-table
      v-if="store.auth"
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
          {{ item.status }}
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

    <v-dialog v-model="modelDialog" max-width="520">
      <v-card>
        <v-card-title class="text-subtitle-1">
          选择测试模型
          <span v-if="modelAccount" class="text-medium-emphasis">· {{ modelAccount.name }}</span>
        </v-card-title>
        <v-card-text>
          <v-progress-linear v-if="loadingModels" indeterminate color="primary" class="mb-2" />
          <div v-else-if="!models.length" class="text-body-2 text-medium-emphasis">
            该账号没有可用的模型列表
          </div>
          <v-list v-else density="compact" max-height="360" class="py-0">
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
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn variant="text" @click="modelDialog = false">取消</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
</template>

<style scoped>
/* 固定表格布局：列宽严格按 header 设定分配，备注列吸收剩余空间。
   自动布局下备注列的不换行长文本会把整张表撑出容器、挤走操作列 */
:deep(.v-data-table table) {
  table-layout: fixed;
}
/* 表头禁止折行，避免窄列标题被挤成两行 */
:deep(.v-data-table__th) {
  white-space: nowrap;
}
:deep(.s2a-row-error) {
  background: rgba(var(--v-theme-error), 0.09);
}
:deep(.s2a-row-warn) {
  background: rgba(var(--v-theme-warning), 0.1);
}
:deep(.s2a-row-active) {
  background: rgba(var(--v-theme-success), 0.08);
}
:deep(tr.s2a-row-error:hover) td,
:deep(.v-data-table__tr.s2a-row-error:hover) td {
  background: rgba(var(--v-theme-error), 0.16) !important;
}
:deep(tr.s2a-row-warn:hover) td,
:deep(.v-data-table__tr.s2a-row-warn:hover) td {
  background: rgba(var(--v-theme-warning), 0.18) !important;
}
:deep(tr.s2a-row-active:hover) td,
:deep(.v-data-table__tr.s2a-row-active:hover) td {
  background: rgba(var(--v-theme-success), 0.14) !important;
}
</style>
