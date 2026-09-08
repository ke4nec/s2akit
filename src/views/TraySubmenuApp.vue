<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import type { AccountBrief, ModelBrief, TestResult } from "../types";

/**
 * 级联子菜单独立窗口：账号操作 / 分组选择两种内容，共用窗口弹出，
 * 主菜单窗口宽度不变、高度不切视图。窗口不可聚焦（不抢焦点），失焦收起走主菜单。
 */
const mode = ref<"account" | "groups">("account");
const account = ref<AccountBrief | null>(null);
const groups = ref<{ id: number; name: string }[]>([]);
const currentGroupId = ref<number | null>(null);
const loadingGroups = ref(false);
/** 正在切行加载中：保留旧内容防闪白，但禁用操作防误触旧账号 */
const pendingId = ref<number | null>(null);
const modelsOpen = ref(false);
const models = ref<ModelBrief[]>([]);
const loadingModels = ref(false);
const testing = ref(false);

function measureHeight(): number {
  const el = document.querySelector<HTMLElement>(".submenu-card");
  if (!el) return 0;
  const extra = el.offsetHeight - el.clientHeight; // 边框等盒模型外扩
  return Math.ceil(Math.max(el.scrollHeight, el.clientHeight) + extra);
}

async function fit() {
  const h = measureHeight();
  if (h <= 0) return;
  try {
    await invoke("fit_submenu", { height: h });
  } catch {
    // 后端命令失败时忽略，下次布局变化重试
  }
}

function scheduleFit() {
  requestAnimationFrame(() => void fit());
}

async function hide() {
  try {
    await invoke("hide_submenu");
  } catch {
    // 忽略
  }
}

/** 模型名截断提示（Vuetify tooltip 做浮层）：仅文本被省略号截断才出现，
 * 悬停 300ms 开显（macOS 提示式节奏），上下空间不够时自动翻面 */
const TOOLTIP_DELAY = 300;
const tipId = ref<string | null>(null);
const tipText = ref("");
const tipLocation = ref<"top" | "bottom">("bottom");
let tipTimer: number | null = null;

function closeTip() {
  if (tipTimer !== null) {
    window.clearTimeout(tipTimer);
    tipTimer = null;
  }
  tipId.value = null;
}

function onModelNameEnter(m: ModelBrief, e: Event) {
  closeTip();
  if (testing.value || pendingId.value !== null) return;
  const el = e.currentTarget as HTMLElement | null;
  if (!el || el.scrollWidth <= el.clientWidth + 1) return;
  tipTimer = window.setTimeout(() => {
    tipTimer = null;
    const r = el.getBoundingClientRect();
    tipText.value = m.display_name || m.id;
    tipLocation.value = r.bottom + 10 + 30 <= window.innerHeight ? "bottom" : "top";
    tipId.value = m.id;
  }, TOOLTIP_DELAY);
}

/** 分组选择模式：自取分组列表与当前选中 */
async function loadGroups() {
  closeTip();
  mode.value = "groups";
  loadingGroups.value = true;
  try {
    groups.value = await invoke<{ id: number; name: string }[]>("list_groups");
    const cfg = await invoke<{ group_id: number | null }>("get_config");
    currentGroupId.value = cfg.group_id;
  } catch {
    groups.value = [];
  } finally {
    loadingGroups.value = false;
    await nextTick();
    scheduleFit();
  }
}

async function onPickGroup(id: number) {
  await hide();
  try {
    await emitTo("tray-menu", "submenu-pick-group", { groupId: id });
  } catch {
    // 忽略
  }
}

/** 主菜单通过后端事件下发账号 id，子菜单自取最新快照（含启停状态） */
async function loadAccount(id: number) {
  closeTip();
  mode.value = "account";
  pendingId.value = id;
  modelsOpen.value = false;
  models.value = [];
  loadingModels.value = false;
  testing.value = false;
  try {
    const list = await invoke<AccountBrief[]>("list_accounts", { groupId: null });
    if (pendingId.value !== id) return;
    const found = list.find((a) => a.id === id) ?? null;
    if (!found) {
      // 切行时账号消失才清空，否则保留旧内容，避免窗口闪白
      if (account.value?.id !== id) account.value = null;
      await hide();
      return;
    }
    account.value = found;
  } catch {
    if (pendingId.value !== id) return;
    // 取数失败保留旧内容（切行动效不断），首次打开失败才收起
    if (account.value === null) {
      await hide();
      return;
    }
  } finally {
    if (pendingId.value === id) pendingId.value = null;
  }
  await nextTick();
  scheduleFit();
}

/** “选择模型测试…”：悬停即展开三级模型列表，点击切换收起 */
async function ensureModels() {
  const a = account.value;
  if (!a || testing.value || modelsOpen.value || pendingId.value !== null) return;
  modelsOpen.value = true;
  models.value = [];
  loadingModels.value = true;
  try {
    models.value = await invoke<ModelBrief[]>("get_account_models", { accountId: a.id });
  } catch {
    modelsOpen.value = false;
  } finally {
    loadingModels.value = false;
    scheduleFit();
  }
}

function toggleModels() {
  if (!account.value || testing.value || pendingId.value !== null) return;
  if (modelsOpen.value) {
    closeTip();
    modelsOpen.value = false;
    models.value = [];
    loadingModels.value = false;
    scheduleFit();
    return;
  }
  void ensureModels();
}

async function onTest(model?: string) {
  const a = account.value;
  if (!a || testing.value || pendingId.value !== null) return;
  testing.value = true;
  try {
    await emitTo("tray-menu", "submenu-test-start", { accountId: a.id });
  } catch {
    // 忽略
  }
  // 原生菜单语义：点选即收起，进度回主菜单行内展示
  await hide();
  let result: TestResult | null = null;
  try {
    result = await invoke<TestResult>("tray_test_account", {
      accountId: a.id,
      model: model ?? null,
    });
  } catch {
    // 失败会走系统通知
  } finally {
    testing.value = false;
  }
  try {
    await emitTo("tray-menu", "submenu-tested", { accountId: a.id, result });
  } catch {
    // 忽略
  }
}

async function onToggle() {
  const a = account.value;
  if (!a || testing.value || pendingId.value !== null) return;
  await hide();
  try {
    await invoke("set_schedulable", {
      accountId: a.id,
      schedulable: !a.schedulable,
    });
    // 成功后后端广播 accounts-updated，主菜单自动刷新
  } catch {
    // 失败会走系统通知
  }
}

async function onEnter() {
  try {
    await emitTo("tray-menu", "submenu-hover", { inside: true });
  } catch {
    // 忽略
  }
}

async function onLeave() {
  try {
    await emitTo("tray-menu", "submenu-hover", { inside: false });
  } catch {
    // 忽略
  }
}

const unlistens: (() => void)[] = [];
onMounted(async () => {
  // 与托盘菜单同一样式底（透明窗口），避免污染主窗口
  document.documentElement.classList.add("s2a-tray-doc");
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
  unlistens.push(
    await listen<number>("submenu-open", (e) => {
      void loadAccount(e.payload);
    })
  );
  unlistens.push(
    await listen("submenu-open-groups", () => {
      void loadGroups();
    })
  );
});
onBeforeUnmount(() => {
  unlistens.forEach((u) => u());
  document.documentElement.classList.remove("s2a-tray-doc");
});
</script>

<template>
  <v-app>
    <v-main class="tray-wrap">
      <v-card
        v-if="mode === 'account' && account"
        elevation="0"
        rounded="0"
        class="submenu-card"
        @mouseenter="onEnter"
        @mouseleave="onLeave"
        @contextmenu.stop.prevent
      >
        <v-list density="compact" class="py-1 bg-transparent">
          <v-list-item
            density="compact"
            :disabled="testing || pendingId !== null"
            @click="onTest()"
            @contextmenu.stop.prevent="onTest()"
          >
            <template #prepend>
              <v-icon icon="mdi-speedometer" size="16" />
            </template>
            <v-list-item-title class="text-caption">测试该账号</v-list-item-title>
          </v-list-item>
          <v-list-item
            density="compact"
            :active="modelsOpen"
            :disabled="testing || pendingId !== null"
            @mouseenter="ensureModels"
            @click="toggleModels"
            @contextmenu.stop.prevent="toggleModels"
          >
            <template #prepend>
              <v-icon icon="mdi-file-tree-outline" size="16" />
            </template>
            <v-list-item-title class="text-caption">选择模型测试…</v-list-item-title>
            <template #append>
              <v-icon icon="mdi-chevron-right" size="14" class="submenu-hint" />
            </template>
          </v-list-item>
          <!-- 三级：模型列表（悬停即加载，内滚，不撑窗口） -->
          <div v-if="modelsOpen" class="submenu-models" @scroll="closeTip">
            <v-progress-linear v-if="loadingModels" indeterminate color="primary" height="2" />
            <v-list-item
              v-else
              density="compact"
              @click="onTest()"
              @contextmenu.stop.prevent="onTest()"
            >
              <template #prepend>
                <v-icon icon="mdi-flash" size="14" color="primary" />
              </template>
              <v-list-item-title class="text-caption">自动选择模型</v-list-item-title>
            </v-list-item>
            <template v-if="!loadingModels">
              <v-list-item
                v-for="m in models"
                :key="m.id"
                density="compact"
                @click="onTest(m.id)"
                @contextmenu.stop.prevent="onTest(m.id)"
              >
                <v-tooltip
                  :model-value="tipId === m.id"
                  :open-on-hover="false"
                  :open-on-click="false"
                  :open-on-focus="false"
                  :location="tipLocation"
                  :offset="6"
                  content-class="apple-tip"
                  transition="fade-transition"
                >
                  <template #activator="{ props }">
                    <v-list-item-title
                      v-bind="props"
                      class="text-caption submenu-model-name"
                      @mouseenter="onModelNameEnter(m, $event)"
                      @mouseleave="closeTip"
                    >
                      {{ m.display_name || m.id }}
                    </v-list-item-title>
                  </template>
                  {{ tipText }}
                </v-tooltip>
              </v-list-item>
              <div v-if="!models.length" class="text-caption text-disabled px-3 py-2">
                该账号没有可用的模型列表
              </div>
            </template>
          </div>
          <v-list-item
            density="compact"
            :disabled="testing || pendingId !== null"
            @click="onToggle"
            @contextmenu.stop.prevent="onToggle"
          >
            <template #prepend>
              <v-icon
                :icon="account.schedulable ? 'mdi-circle-outline' : 'mdi-circle'"
                size="16"
              />
            </template>
            <v-list-item-title class="text-caption">
              {{ account.schedulable ? "禁用该账号" : "启用该账号" }}
            </v-list-item-title>
          </v-list-item>
        </v-list>
      </v-card>
      <v-card
        v-if="mode === 'groups'"
        elevation="0"
        rounded="0"
        class="submenu-card"
        @mouseenter="onEnter"
        @mouseleave="onLeave"
        @contextmenu.stop.prevent
      >
        <v-progress-linear v-if="loadingGroups" indeterminate color="primary" height="2" />
        <v-list density="compact" class="py-1 bg-transparent">
          <v-list-item
            v-for="g in groups"
            :key="g.id"
            density="compact"
            @click="onPickGroup(g.id)"
            @contextmenu.stop.prevent="onPickGroup(g.id)"
          >
            <template #prepend>
              <v-icon
                :icon="g.id === currentGroupId ? 'mdi-check' : 'mdi-circle-medium'"
                :color="g.id === currentGroupId ? 'primary' : 'grey'"
                size="14"
              />
            </template>
            <v-list-item-title class="text-caption">{{ g.name }}</v-list-item-title>
          </v-list-item>
          <div v-if="!loadingGroups && !groups.length" class="text-caption text-disabled px-3 py-2">
            暂无分组
          </div>
        </v-list>
      </v-card>
    </v-main>
  </v-app>
</template>

<style scoped>
/* 卡片铺满子菜单窗口；与主菜单同一套 Apple 质感（Acrylic + 发丝边框 + 内高光） */
.tray-wrap {
  padding: 0 !important;
}
.submenu-card {
  position: relative;
  background: rgba(248, 248, 250, 0.78);
  max-height: 100vh;
  border: 1px solid rgba(0, 0, 0, 0.1);
  /* 8px 与 DWM 窗口圆角一致，与主菜单严丝合缝 */
  border-radius: 8px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55);
  padding: 2px;
}
/* 加载条悬浮顶部不占布局，避免窗口高度抖动 */
.submenu-card > .v-progress-linear {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  z-index: 1;
}
.submenu-card .v-list-item {
  --v-list-item-one-line-height: 32px;
  min-height: 32px;
  border-radius: 6px;
  margin: 0 1px;
}
.submenu-card .v-list-item .v-list-item-title {
  font-size: 12px !important;
  color: rgba(0, 0, 0, 0.85);
}
.submenu-card .v-list-item__overlay {
  border-radius: 6px;
}
/* 子菜单指示箭头：常态弱化，悬浮时加深 */
.submenu-hint {
  color: rgba(0, 0, 0, 0.3);
}
.v-list-item:hover .submenu-hint,
.v-list-item--active .submenu-hint {
  color: rgba(0, 0, 0, 0.55);
}
/* 三级模型列表：内滚，不撑大窗口 */
.submenu-models {
  max-height: 192px;
  overflow-y: auto;
  border-top: 1px solid rgba(0, 0, 0, 0.06);
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
  margin: 2px 1px;
}
.submenu-model-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 模型名截断提示：Vuetify tooltip 做浮层，此处只覆盖苹果风深色皮肤 */
.v-overlay__content.apple-tip {
  background: rgba(28, 28, 30, 0.94);
  color: #fff;
  font-size: 12px;
  line-height: 1.5;
  padding: 4px 9px;
  border-radius: 7px;
  max-width: 280px;
  word-break: break-all;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.25);
}
</style>
