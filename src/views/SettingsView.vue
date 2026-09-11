<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { checkForUpdates, installUpdate, login, saveConfig, store, submit2fa } from "../store";
import { selectTheme, selectThemeFx, themeFxOptions, themeFxState, themeOptions, themeState } from "../theme";
import type { ThemeFx } from "../theme";
import type { AppConfig } from "../types";

function defaultConfig(): AppConfig {
  return {
    base_url: "http://127.0.0.1:8080",
    email: "",
    password: "",
    group_id: null,
    test_prompt: "hi",
    test_timeout_secs: 60,
    test_concurrency: 2,
    menu_opacity: 1,
    usage_refresh_minutes: 10,
    theme: "light",
    theme_fx: "reveal",
  };
}

const form = reactive<AppConfig>(defaultConfig());

watch(
  () => store.config,
  (c) => {
    if (c) Object.assign(form, JSON.parse(JSON.stringify(c)));
  },
  { immediate: true },
);

const totpCode = ref("");

const opacityPercent = computed(() => Math.round(form.menu_opacity * 100));
/** 滑杆轨道已走过比例（30-100 映射为 0-100%），驱动渐变填充 */
const opacityFill = computed(() => `${(((opacityPercent.value - 30) / 70) * 100).toFixed(2)}%`);

/** 托盘菜单不透明度：步进 1 时更新频繁，防抖后再持久化+应用，拖动才不卡 */
let opacityTimer: number | undefined;

function persistMenuOpacity() {
  void invoke("set_menu_opacity", { opacity: form.menu_opacity }).catch(() => {});
}

function onOpacityInput(e: Event) {
  form.menu_opacity = Number((e.target as HTMLInputElement).value) / 100;
  window.clearTimeout(opacityTimer);
  opacityTimer = window.setTimeout(persistMenuOpacity, 150);
}

onBeforeUnmount(() => {
  if (opacityTimer !== undefined) {
    window.clearTimeout(opacityTimer);
    persistMenuOpacity();
  }
});

async function doSave() {
  if (!form.base_url.trim()) {
    form.base_url = "http://127.0.0.1:8080";
  }
  form.test_timeout_secs = Math.max(5, Number(form.test_timeout_secs) || 60);
  form.test_concurrency = Math.min(8, Math.max(1, Number(form.test_concurrency) || 2));
  form.usage_refresh_minutes = Math.min(1440, Math.max(1, Math.round(Number(form.usage_refresh_minutes) || 10)));
  await saveConfig({ ...form });
}

async function doSaveAndLogin() {
  await doSave();
  await login();
}

async function doSubmit2fa() {
  if (!totpCode.value.trim()) return;
  await submit2fa(totpCode.value.trim());
}

/** 当前所选动效的说明文案：设置页「切换动效」行下方展示 */
const activeFxDesc = computed(() => themeFxOptions.find((o) => o.value === themeFxState.fx)?.desc ?? "");

/** 当前所选动效的展示名：下拉触发按钮文案（与主页面 Group 下拉同款） */
const activeFxLabel = computed(() => themeFxOptions.find((o) => o.value === themeFxState.fx)?.label ?? "");

/** 切换动效下拉变更：即时生效（下次切主题时用）+ 持久化 + 跨窗口同步 */
function onFxSelect(fx: ThemeFx) {
  selectThemeFx(fx);
}

/** 更新状态提示：下载中/已就绪时显示在按钮左侧 */
const updateHint = computed(() => {
  const u = store.updater;
  if (u.status === "downloading") return `正在下载 v${u.version}…`;
  if (u.status === "ready") return `v${u.version} 已就绪`;
  return "";
});

function onCheckUpdate() {
  // 已有校验通过的更新时按钮变为「立即安装」，否则发起检查（有新版会弹统一对话框）
  if (store.updater.status === "ready") void installUpdate();
  else void checkForUpdates(true);
}
</script>

<template>
  <div class="settings">
    <!-- 分组一：服务与管理员账号 -->
    <section class="group animate-apple-fade-in">
      <header class="group-head">
        <v-icon icon="mdi-server" size="18" />
        <div class="group-head-text">
          <div class="group-title">服务与管理员账号</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label">sub2api 服务地址</div>
        <div class="ctrl">
          <input
            v-model="form.base_url"
            class="ctrl-input"
            type="text"
            aria-label="sub2api 服务地址"
            placeholder="http://127.0.0.1:8080"
            spellcheck="false"
          />
        </div>
      </div>
      <div class="row">
        <div class="row-label">管理员邮箱</div>
        <div class="ctrl">
          <input
            v-model="form.email"
            class="ctrl-input"
            type="email"
            aria-label="管理员邮箱"
            placeholder="admin@example.com"
            spellcheck="false"
          />
        </div>
      </div>
      <div class="row">
        <div class="row-label">密码</div>
        <div class="ctrl">
          <input v-model="form.password" class="ctrl-input" type="password" aria-label="密码" placeholder="管理员密码" />
        </div>
      </div>
      <div v-if="store.pending2fa" class="row">
        <div class="row-label">二步验证码</div>
        <div class="ctrl">
          <input
            v-model="totpCode"
            class="ctrl-input"
            type="text"
            aria-label="6 位验证码"
            maxlength="6"
            inputmode="numeric"
            placeholder="6 位验证码"
            @keyup.enter="doSubmit2fa"
          />
        </div>
      </div>
      <footer class="group-foot">
        <v-btn
          :variant="store.pending2fa ? 'tonal' : 'flat'"
          color="primary"
          :loading="store.loggingIn || store.saving"
          @click="doSaveAndLogin"
        >
          登录
        </v-btn>
        <v-btn v-if="store.pending2fa" color="primary" @click="doSubmit2fa">验证并登录</v-btn>
      </footer>
    </section>

    <!-- 分组二：测速参数 -->
    <section class="group animate-apple-fade-in apple-delay-1">
      <header class="group-head">
        <v-icon icon="mdi-speedometer" size="18" />
        <div class="group-head-text">
          <div class="group-title">测速参数</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label" title="发给模型的提示词，建议保持极短">测试 Prompt</div>
        <div class="ctrl">
          <input v-model="form.test_prompt" class="ctrl-input" type="text" aria-label="测试 Prompt" spellcheck="false" />
        </div>
      </div>
      <div class="row">
        <div class="row-label">单次测试超时（秒）</div>
        <div class="ctrl">
          <input v-model.number="form.test_timeout_secs" class="ctrl-input" type="number" aria-label="单次测试超时（秒）" min="5" />
        </div>
      </div>
      <div class="row row--disabled">
        <div class="row-label">批量测试并发数（1-8）</div>
        <div class="ctrl">
          <input v-model.number="form.test_concurrency" class="ctrl-input" type="number" aria-label="批量测试并发数（1-8）" min="1" max="8" disabled />
        </div>
      </div>
    </section>

    <!-- 分组三：外观 -->
    <section class="group animate-apple-fade-in apple-delay-2">
      <header class="group-head">
        <svg
          class="theme-mark"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="9" />
          <path d="M12 3a9 9 0 0 1 0 18Z" fill="currentColor" stroke="none" />
        </svg>
        <div class="group-head-text">
          <div class="group-title">外观</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label">界面主题</div>
        <div class="ctrl">
          <div class="theme-seg" role="radiogroup" aria-label="界面主题">
            <button
              v-for="opt in themeOptions"
              :key="opt.value"
              type="button"
              role="radio"
              class="theme-seg-btn"
              :class="{ 'theme-seg-btn--active': themeState.pref === opt.value }"
              :aria-checked="themeState.pref === opt.value"
              @click="selectTheme(opt.value, $event)"
            >
              {{ opt.label }}
            </button>
          </div>
        </div>
      </div>
      <div class="row row--fx">
        <div class="row-label">切换动效</div>
        <div class="ctrl ctrl--fx">
          <!-- 动效下拉：与主页面 Group 选择器同款 v-menu 胶囊（替代原生 select，各主题下质感一致） -->
          <v-menu scroll-strategy="close">
            <template #activator="{ props: menuProps }">
              <button
                v-bind="menuProps"
                type="button"
                class="fx-pop"
                aria-label="主题切换动效"
              >
                <span class="fx-pop-text">{{ activeFxLabel }}</span>
                <v-icon icon="mdi-chevron-down" size="15" class="fx-pop-chevron" />
              </button>
            </template>
            <v-list density="compact" class="fx-list">
              <v-list-item v-for="fx in themeFxOptions" :key="fx.value" @click="onFxSelect(fx.value)">
                <template #prepend>
                  <v-icon
                    :icon="fx.value === themeFxState.fx ? 'mdi-check' : 'mdi-circle-medium'"
                    :color="fx.value === themeFxState.fx ? 'primary' : 'grey'"
                    size="15"
                  />
                </template>
                <v-list-item-title>{{ fx.label }}</v-list-item-title>
              </v-list-item>
            </v-list>
          </v-menu>
          <div v-if="activeFxDesc" class="fx-desc">{{ activeFxDesc }}</div>
        </div>
      </div>
    </section>

    <!-- 分组四：托盘菜单 -->
    <section class="group animate-apple-fade-in apple-delay-3">
      <header class="group-head">
        <v-icon icon="mdi-dock-right" size="18" />
        <div class="group-head-text">
          <div class="group-title">托盘菜单</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label">任务栏菜单不透明度</div>
        <div class="ctrl slider-wrap">
          <span class="slider-value">{{ opacityPercent }}%</span>
          <input
            class="ctrl-slider"
            type="range"
            aria-label="任务栏菜单不透明度"
            min="30"
            max="100"
            step="1"
            :value="opacityPercent"
            :style="{ '--p': opacityFill }"
            @input="onOpacityInput"
          />
        </div>
      </div>
      <div class="row">
        <div
          class="row-label"
          title="托盘菜单顶部额度数据的缓存时长，间隔内重复打开菜单不再请求服务端；范围 1-1440，默认 10"
        >
          额度刷新间隔（分钟）
        </div>
        <div class="ctrl">
          <input v-model.number="form.usage_refresh_minutes" class="ctrl-input" type="number" aria-label="额度刷新间隔（分钟）" min="1" max="1440" />
        </div>
      </div>
    </section>

    <!-- 分组五：关于 -->
    <section class="group animate-apple-fade-in apple-delay-3">
      <header class="group-head">
        <v-icon icon="mdi-information" size="18" />
        <div class="group-head-text">
          <div class="group-title">关于</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label">当前版本</div>
        <div class="ctrl">
          <span class="about-version">v{{ store.updater.current || "…" }}</span>
        </div>
      </div>
    </section>

    <!-- 页面底部操作：检查更新与保存同一行 -->
    <div class="save-bar">
      <span v-if="updateHint" class="update-hint">{{ updateHint }}</span>
      <v-btn
        color="primary"
        variant="tonal"
        :loading="store.updater.checking"
        :disabled="store.updater.status === 'downloading'"
        @click="onCheckUpdate"
      >
        {{ store.updater.status === "ready" ? "立即安装" : "检查更新" }}
      </v-btn>
      <v-btn color="primary" :loading="store.saving" @click="doSave">保存</v-btn>
    </div>
  </div>
</template>

<style scoped>
/* 设计稿 design/settings.html：单列居中分组 · 左标签右控件 · 填充式输入 */
.settings {
  max-width: 780px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.group {
  background: hsl(var(--background));
  border: 1px solid hsl(var(--divider));
  border-radius: 8px;
  box-shadow: var(--card-shadow);
}

.group-head {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 16px 20px 14px;
}
.group-head > .v-icon {
  margin-top: 1px;
  color: hsl(var(--muted-foreground));
}
.theme-mark {
  flex: none;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  color: hsl(var(--muted-foreground));
}
.group-head-text {
  min-width: 0;
}
.group-title {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
  line-height: 1.3;
  color: hsl(var(--foreground));
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  min-height: 48px;
  padding: 10px 20px;
}
.row + .row {
  border-top: 1px solid hsl(var(--divider));
}
.row-label {
  flex: none;
  width: 190px;
  font-size: 14px;
  color: hsl(var(--foreground));
  line-height: 1.4;
}

/* 填充式输入：浅底无边框，聚焦卡面 + 蓝描边光晕 */
.ctrl {
  flex: none;
}
.ctrl-input {
  width: 160px;
  max-width: 100%;
  height: 34px;
  padding: 0 12px;
  background: hsl(var(--surface-2));
  border: 1px solid transparent;
  border-radius: 6px;
  font-family: inherit;
  font-size: 13px;
  color: hsl(var(--foreground));
  outline: none;
  transition:
    background 0.2s var(--ease-in-out),
    border-color 0.2s var(--ease-in-out),
    box-shadow 0.2s var(--ease-in-out);
}
.ctrl-input::placeholder {
  color: hsl(var(--text-placeholder));
}
.ctrl-input:hover {
  background: hsl(var(--surface-3));
}
.ctrl-input:focus {
  background: hsl(var(--background));
  border-color: hsl(var(--accent));
  box-shadow: 0 0 0 3px hsl(var(--accent) / 0.12);
}
.ctrl-input:focus-visible {
  outline: none;
}
/* 未开放的设置项：输入框禁用态 + 行级弱化（无额外文案） */
.ctrl-input:disabled {
  background: hsl(var(--surface-3));
  color: hsl(var(--text-placeholder));
  cursor: not-allowed;
}
.ctrl-input:disabled:hover {
  background: hsl(var(--surface-3));
}
.row--disabled .row-label {
  color: hsl(var(--muted-foreground));
}

/* 主题三选分段控件：与标题栏导航同款 macOS 规格 */
.theme-seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: 9px;
  background: var(--seg-fill);
}
.theme-seg-btn {
  height: 26px;
  padding: 0 14px;
  border: none;
  border-radius: 7px;
  background: transparent;
  font-family: inherit;
  font-size: 13px;
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  transition:
    color 0.2s var(--ease-in-out),
    background 0.2s var(--ease-in-out),
    box-shadow 0.2s var(--ease-in-out);
}
.theme-seg-btn:hover {
  color: hsl(var(--foreground));
}
.theme-seg-btn--active {
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  font-weight: 600;
  box-shadow: var(--seg-active-shadow);
}
/* 切换动效行：下拉与说明文案纵向堆叠右对齐，标签顶部对齐 */
.row--fx {
  align-items: flex-start;
}
.ctrl--fx {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
  min-width: 0;
}
/* 动效下拉触发胶囊：与主页面 Group 选择器（AccountsView .group-pop）同款，
   macOS 工具栏弹出菜单风格，28px 高、灰底圆角、悬浮加深；定宽与输入框右对齐 */
.fx-pop {
  flex: none;
  width: 160px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 3px;
  min-width: 0;
  padding: 0 7px 0 11px;
  border: none;
  border-radius: 7px;
  background: var(--seg-fill);
  font-family: inherit;
  font-size: 12px;
  font-weight: 500;
  letter-spacing: -0.01em;
  color: hsl(var(--foreground) / 0.78);
  cursor: pointer;
  user-select: none;
  transition: background 0.15s var(--ease-in-out, ease);
}
.fx-pop:hover {
  background: var(--seg-fill-strong);
}
.fx-pop:focus-visible {
  outline: 2px solid hsl(var(--accent) / 0.45);
}
.fx-pop-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fx-pop-chevron {
  flex: none;
  color: hsl(var(--foreground) / 0.4);
}
/* 动效下拉列表：与主页面 Group 下拉（AccountsView .group-list）一致，Vuetify 菜单默认质感 */
.fx-list {
  max-height: 360px;
  overflow-y: auto;
}
.fx-desc {
  font-size: 12px;
  line-height: 1.4;
  color: hsl(var(--muted-foreground));
  text-align: right;
}

/* 滑杆：数值置于轨道上方，省横向宽度；蓝填充轨道 + 白圆钮 */
.slider-wrap {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 2px;
}
.ctrl-slider {
  appearance: none;
  -webkit-appearance: none;
  /* 与文本输入框同宽（160px），右侧控件纵向对齐 */
  width: 160px;
  height: 6px;
  border-radius: 3px;
  background: linear-gradient(to right, hsl(var(--accent)) var(--p), hsl(var(--divider)) var(--p));
  outline: none;
  cursor: pointer;
}
.ctrl-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  border: 2px solid hsl(var(--accent));
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
  transition: transform 0.15s var(--ease-out);
}
.ctrl-slider::-webkit-slider-thumb:hover {
  transform: scale(1.15);
}
.ctrl-slider::-moz-range-thumb {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: #fff;
  border: 2px solid hsl(var(--accent));
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
}
.ctrl-slider:focus-visible {
  box-shadow: 0 0 0 3px hsl(var(--accent) / 0.12);
}
.slider-value {
  align-self: flex-end;
  font-size: 13px;
  font-weight: 600;
  color: hsl(var(--accent));
  font-variant-numeric: tabular-nums;
}

/* 组底操作区与按钮 */
.group-foot {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 20px 16px;
  border-top: 1px solid hsl(var(--divider));
}
.group-foot .v-btn,
.save-bar .v-btn {
  height: 32px;
  min-width: 72px;
  padding: 0 15px;
  border-radius: 6px;
  font-size: 13px;
}

.save-bar {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 12px;
}
.save-bar .v-btn {
  height: 34px;
  min-width: 96px;
}

/* 关于分组：版本号与更新操作 */
.about-version {
  font-size: 13px;
  font-weight: 600;
  color: hsl(var(--foreground));
  font-variant-numeric: tabular-nums;
}
.update-hint {
  font-size: 13px;
  color: hsl(var(--muted-foreground));
}

/* 窄窗兜底：标签与控件上下堆叠（正常窗口 minWidth 880 不会触发） */
@media (max-width: 640px) {
  .row {
    flex-wrap: wrap;
  }
  .ctrl {
    flex: 1;
    min-width: 0;
  }
  .ctrl-input,
  .ctrl-slider {
    width: 100%;
  }
}
</style>
