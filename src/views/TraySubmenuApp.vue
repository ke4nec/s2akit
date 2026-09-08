<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import type { AccountBrief, ModelBrief, TestResult } from "../types";

/**
 * 账号级联子菜单独立窗口：主菜单悬停/点击账户行时另起窗口弹出，
 * 主菜单窗口宽度不变。窗口不可聚焦（不抢焦点），失焦收起走主菜单。
 */
const account = ref<AccountBrief | null>(null);
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

/** 主菜单通过后端事件下发账号 id，子菜单自取最新快照（含启停状态） */
async function loadAccount(id: number) {
  account.value = null;
  modelsOpen.value = false;
  models.value = [];
  loadingModels.value = false;
  testing.value = false;
  try {
    const list = await invoke<AccountBrief[]>("list_accounts", { groupId: null });
    account.value = list.find((a) => a.id === id) ?? null;
  } catch {
    account.value = null;
  }
  if (!account.value) {
    await hide();
    return;
  }
  await nextTick();
  scheduleFit();
}

/** “选择模型测试…”：悬停即展开三级模型列表，点击切换收起 */
async function ensureModels() {
  const a = account.value;
  if (!a || testing.value || modelsOpen.value) return;
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
  if (!account.value || testing.value) return;
  if (modelsOpen.value) {
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
  if (!a || testing.value) return;
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
  if (!a || testing.value) return;
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
        v-if="account"
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
            :disabled="testing"
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
            :disabled="testing"
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
          <div v-if="modelsOpen" class="submenu-models">
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
                <template #prepend>
                  <v-icon icon="mdi-chat-processing-outline" size="14" color="grey" />
                </template>
                <v-list-item-title class="text-caption submenu-model-name">
                  {{ m.display_name || m.id }}
                </v-list-item-title>
              </v-list-item>
              <div v-if="!models.length" class="text-caption text-disabled px-3 py-2">
                该账号没有可用的模型列表
              </div>
            </template>
          </div>
          <v-list-item
            density="compact"
            :disabled="testing"
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
    </v-main>
  </v-app>
</template>

<style scoped>
/* 卡片铺满子菜单窗口；与主菜单同一套 Apple 质感（Acrylic + 发丝边框 + 内高光） */
.tray-wrap {
  padding: 0 !important;
}
.submenu-card {
  background: rgba(248, 248, 250, 0.78);
  max-height: 100vh;
  border: 1px solid rgba(0, 0, 0, 0.1);
  /* 8px 与 DWM 窗口圆角一致，与主菜单严丝合缝 */
  border-radius: 8px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55);
  padding: 2px;
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
</style>
