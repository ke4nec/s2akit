<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { initTheme } from "../theme";
import type { AccountBrief, AppConfig, KeyBrief, ModelBrief, TestResult } from "../types";

/**
 * 级联子菜单独立窗口：账号操作 / Key 操作 / 分组选择三种内容，共用窗口弹出，
 * 主菜单窗口宽度不变、高度不切视图。窗口不可聚焦（不抢焦点），失焦收起走主菜单。
 */
const mode = ref<"account" | "key" | "groups">("account");
const account = ref<AccountBrief | null>(null);
/** Key 操作模式状态（与账号态独立存放，避免 id 重叠串台） */
const keyBrief = ref<KeyBrief | null>(null);
const pendingKeyId = ref<number | null>(null);
const keyModelsOpen = ref(false);
const keyModels = ref<ModelBrief[]>([]);
const keyLoadingModels = ref(false);
const keyTesting = ref(false);
let keyHoverModelsTimer: number | undefined;
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
  // 兜底：字体/列表渲染晚于首帧的高度变化，延迟再校一次（fit 幂等，仅按内容收敛）
  window.setTimeout(() => void fit(), 160);
}

/** 悬停提示透明度跟随菜单不透明度设置（×85%），每次打开子菜单刷新一次 */
async function applyTipAlpha() {
  try {
    const cfg = await invoke<AppConfig>("get_config");
    const alpha = Math.min(1, Math.max(0, cfg.menu_opacity * 0.85));
    document.documentElement.style.setProperty("--tip-alpha", alpha.toFixed(3));
  } catch {
    // 忽略，保持默认
  }
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

/** 该 Key 是否为顶部用量统计源（勾选态） */
const usageSelected = ref(false);

/** Key 操作模式：自取 key 快照（含测试用的 secret），保留旧内容防闪白 */
async function loadKey(id: number) {
  closeTip();
  window.clearTimeout(hoverModelsTimer);
  void applyTipAlpha();
  mode.value = "key";
  usageSelected.value = false;
  try {
    usageSelected.value = (await invoke<number | null>("get_usage_key").catch(() => null)) === id;
  } catch {
    // 忽略
  }
  pendingKeyId.value = id;
  keyModelsOpen.value = false;
  keyModels.value = [];
  keyLoadingModels.value = false;
  keyTesting.value = false;
  try {
    const list = await invoke<KeyBrief[]>("list_keys");
    if (pendingKeyId.value !== id) return;
    const found = list.find((k) => k.id === id) ?? null;
    if (!found) {
      if (keyBrief.value?.id !== id) keyBrief.value = null;
      await hide();
      return;
    }
    keyBrief.value = found;
  } catch {
    if (pendingKeyId.value !== id) return;
    if (keyBrief.value === null) {
      await hide();
      return;
    }
  } finally {
    if (pendingKeyId.value === id) pendingKeyId.value = null;
  }
  await nextTick();
  scheduleFit();
}

/** Key 的“选择模型测试…”：悬停约 200ms（意图延迟，划过不展开）或点击切换 */
function hoverKeyModels() {
  window.clearTimeout(keyHoverModelsTimer);
  keyHoverModelsTimer = window.setTimeout(() => void ensureKeyModels(), 200);
}

function cancelHoverKeyModels() {
  window.clearTimeout(keyHoverModelsTimer);
}

async function ensureKeyModels() {
  const k = keyBrief.value;
  if (!k || keyTesting.value || keyModelsOpen.value || pendingKeyId.value !== null) return;
  keyModelsOpen.value = true;
  keyModels.value = [];
  keyLoadingModels.value = true;
  try {
    keyModels.value = await invoke<ModelBrief[]>("get_key_models", { keySecret: k.key });
  } catch {
    keyModelsOpen.value = false;
  } finally {
    keyLoadingModels.value = false;
    scheduleFit();
  }
}

function toggleKeyModels() {
  if (!keyBrief.value || keyTesting.value || pendingKeyId.value !== null) return;
  if (keyModelsOpen.value) {
    closeTip();
    window.clearTimeout(keyHoverModelsTimer);
    keyModelsOpen.value = false;
    keyModels.value = [];
    keyLoadingModels.value = false;
    scheduleFit();
    return;
  }
  void ensureKeyModels();
}

async function onTestKey(model?: string) {
  const k = keyBrief.value;
  if (!k || keyTesting.value || pendingKeyId.value !== null) return;
  keyTesting.value = true;
  try {
    await emitTo("tray-menu", "submenu-test-start", { kind: "key", accountId: k.id });
  } catch {
    // 忽略
  }
  // 原生菜单语义：点选即收起，进度回主菜单行内展示
  await hide();
  let result: TestResult | null = null;
  try {
    result = await invoke<TestResult>("tray_test_key", {
      keyId: k.id,
      keyName: k.name,
      keySecret: k.key,
      model: model ?? null,
    });
  } catch {
    // 失败会走系统通知
  } finally {
    keyTesting.value = false;
  }
  try {
    await emitTo("tray-menu", "submenu-tested", { kind: "key", accountId: k.id, result });
  } catch {
    // 忽略
  }
}

/** “查看额度”：切为该 Key 即切换统计源，再点一次恢复汇总全部 */
async function onPickUsage() {
  const k = keyBrief.value;
  if (!k || keyTesting.value || pendingKeyId.value !== null) return;
  await hide();
  try {
    await invoke("set_usage_key", { keyId: usageSelected.value ? null : k.id });
  } catch {
    // 忽略
  }
  try {
    await emitTo("tray-menu", "submenu-usage-changed", {});
  } catch {
    // 忽略
  }
}

async function onToggleKey() {
  const k = keyBrief.value;
  if (!k || keyTesting.value || pendingKeyId.value !== null) return;
  await hide();
  try {
    // 命令返回刷新后的列表，顺事件带回，主菜单直接用，不再多拉一次
    const keys = await invoke<KeyBrief[]>("set_key_enabled", {
      keyId: k.id,
      enabled: k.status !== "active",
    });
    await emitTo("tray-menu", "submenu-key-toggled", { keys });
  } catch {
    // 失败服务端无变更，主菜单保持原状态即可
  }
}

/** 分组选择模式：自取分组列表与当前选中 */
async function loadGroups() {
  closeTip();
  window.clearTimeout(hoverModelsTimer);
  window.clearTimeout(keyHoverModelsTimer);
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
  window.clearTimeout(keyHoverModelsTimer);
  void applyTipAlpha();
  mode.value = "account";
  pendingId.value = id;
  modelsOpen.value = false;
  models.value = [];
  loadingModels.value = false;
  testing.value = false;
  window.clearTimeout(hoverModelsTimer);
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

/** “选择模型测试…”：悬停约 200ms（意图延迟，划过不展开）或点击切换收起 */
let hoverModelsTimer: number | undefined;

function hoverModels() {
  window.clearTimeout(hoverModelsTimer);
  hoverModelsTimer = window.setTimeout(() => void ensureModels(), 200);
}

function cancelHoverModels() {
  window.clearTimeout(hoverModelsTimer);
}

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
    window.clearTimeout(hoverModelsTimer);
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
    await emitTo("tray-menu", "submenu-test-start", { kind: "account", accountId: a.id });
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
    await emitTo("tray-menu", "submenu-tested", { kind: "account", accountId: a.id, result });
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
  // 子菜单窗口不跑 store.init()，主题按配置自行初始化（含系统明暗/跨窗口监听）
  invoke<AppConfig>("get_config")
    .then((cfg) => void initTheme(cfg.theme))
    .catch(() => {});
  unlistens.push(
    await listen<{ kind: string; id: number }>("submenu-open", (e) => {
      if (e.payload.kind === "key") void loadKey(e.payload.id);
      else void loadAccount(e.payload.id);
    })
  );
  unlistens.push(
    await listen("submenu-open-groups", () => {
      void loadGroups();
    })
  );
});
onBeforeUnmount(() => {
  window.clearTimeout(hoverModelsTimer);
  window.clearTimeout(keyHoverModelsTimer);
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
            :active="modelsOpen"
            :disabled="testing || pendingId !== null"
            @mouseenter="hoverModels"
            @mouseleave="cancelHoverModels"
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
          <!-- 原生菜单惯例：启停与其他项分隔 -->
          <v-divider />
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
        v-if="mode === 'key' && keyBrief"
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
            :active="usageSelected"
            :disabled="keyTesting || pendingKeyId !== null"
            @click="onPickUsage"
            @contextmenu.stop.prevent="onPickUsage"
          >
            <template #prepend>
              <v-icon icon="mdi-chart-areaspline" size="16" />
            </template>
            <v-list-item-title class="text-caption">
              {{ usageSelected ? "✓ 正在查看该 Key" : "查看该 Key 额度" }}
            </v-list-item-title>
          </v-list-item>
          <v-list-item
            density="compact"
            :active="keyModelsOpen"
            :disabled="keyTesting || pendingKeyId !== null"
            @mouseenter="hoverKeyModels"
            @mouseleave="cancelHoverKeyModels"
            @click="toggleKeyModels"
            @contextmenu.stop.prevent="toggleKeyModels"
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
          <div v-if="keyModelsOpen" class="submenu-models" @scroll="closeTip">
            <v-progress-linear v-if="keyLoadingModels" indeterminate color="primary" height="2" />
            <v-list-item
              v-else
              density="compact"
              @click="onTestKey()"
              @contextmenu.stop.prevent="onTestKey()"
            >
              <template #prepend>
                <v-icon icon="mdi-flash" size="14" color="primary" />
              </template>
              <v-list-item-title class="text-caption">自动选择模型</v-list-item-title>
            </v-list-item>
            <template v-if="!keyLoadingModels">
              <v-list-item
                v-for="m in keyModels"
                :key="m.id"
                density="compact"
                @click="onTestKey(m.id)"
                @contextmenu.stop.prevent="onTestKey(m.id)"
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
              <div v-if="!keyModels.length" class="text-caption text-disabled px-3 py-2">
                该 Key 没有可用的模型列表
              </div>
            </template>
          </div>
          <!-- 原生菜单惯例：启停与其他项分隔 -->
          <v-divider />
          <v-list-item
            density="compact"
            :disabled="keyTesting || pendingKeyId !== null"
            @click="onToggleKey"
            @contextmenu.stop.prevent="onToggleKey"
          >
            <template #prepend>
              <v-icon
                :icon="keyBrief.status === 'active' ? 'mdi-circle-outline' : 'mdi-circle'"
                size="16"
              />
            </template>
            <v-list-item-title class="text-caption">
              {{ keyBrief.status === "active" ? "禁用该 Key" : "启用该 Key" }}
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
/* 卡片铺满子菜单窗口；与主菜单同一套规格（Acrylic 底、发丝边框、内高光） */
.tray-wrap {
  padding: 0 !important;
}
.submenu-card {
  position: relative;
  /* macOS 式弹出：淡入 + 轻微放大（v-if 重建时重播，切换内容也有跟手感） */
  animation: sub-in 0.12s ease-out;
  background: rgba(246, 246, 248, 0.72);
  max-height: 100vh;
  border: 1px solid rgba(0, 0, 0, 0.08);
  /* 8px 与 DWM 窗口圆角一致，与主菜单严丝合缝 */
  border-radius: 8px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55);
}
@keyframes sub-in {
  from {
    opacity: 0;
    transform: scale(0.97);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}
@media (prefers-reduced-motion: reduce) {
  .submenu-card {
    animation: none;
  }
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
  /* prepend 间距收紧到 8px（Vuetify 图标后 spacer 默认 32px，过宽不像原生菜单），与主菜单一致 */
  --v-list-prepend-gap: 8px;
  min-height: 32px;
  border-radius: 7px;
  margin: 0 2px;
}
.submenu-card .v-list-item .v-list-item-title {
  font-size: 12px !important;
  color: rgba(0, 0, 0, 0.85);
}
.submenu-card .v-list-item__overlay {
  border-radius: 7px;
}
.submenu-card .v-divider {
  border-color: rgba(0, 0, 0, 0.1) !important;
  opacity: 1 !important;
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
  border-top: 1px solid rgba(0, 0, 0, 0.08);
  border-bottom: 1px solid rgba(0, 0, 0, 0.08);
  margin: 2px;
}
.submenu-model-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---------- 暗色主题：与主菜单同一套 Apple 深色覆盖（html.dark 由主题切换注入）；
   acrylic 玻璃底色由 Rust 侧 set_effects 同步切换，此处只管网页层 ---------- */
html.s2a-tray-doc.dark .submenu-card {
  background: rgba(30, 30, 32, 0.72);
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.06);
}
html.s2a-tray-doc.dark .submenu-card .v-list-item .v-list-item-title {
  color: rgba(255, 255, 255, 0.92);
}
html.s2a-tray-doc.dark .submenu-card .v-divider {
  border-color: rgba(255, 255, 255, 0.12) !important;
}
html.s2a-tray-doc.dark .submenu-hint {
  color: rgba(255, 255, 255, 0.3);
}
html.s2a-tray-doc.dark .v-list-item:hover .submenu-hint,
html.s2a-tray-doc.dark .v-list-item--active .submenu-hint {
  color: rgba(255, 255, 255, 0.55);
}
html.s2a-tray-doc.dark .submenu-models {
  border-top-color: rgba(255, 255, 255, 0.08);
  border-bottom-color: rgba(255, 255, 255, 0.08);
}
</style>
