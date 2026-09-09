<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { checkForUpdates, installUpdate, login, saveConfig, store, submit2fa } from "../store";
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
          <div class="group-sub">连接 sub2api 服务，凭据仅保存在本机配置文件</div>
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
            class="ctrl-input ctrl-input--narrow"
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
          <div class="group-sub">控制账号批量测速的行为与判定</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label" title="发给模型的提示词，建议保持极短">测试 Prompt</div>
        <div class="ctrl">
          <input v-model="form.test_prompt" class="ctrl-input ctrl-input--narrow" type="text" aria-label="测试 Prompt" spellcheck="false" />
        </div>
      </div>
      <div class="row">
        <div class="row-label">单次测试超时（秒）</div>
        <div class="ctrl">
          <input v-model.number="form.test_timeout_secs" class="ctrl-input ctrl-input--narrow" type="number" aria-label="单次测试超时（秒）" min="5" />
        </div>
      </div>
      <div class="row">
        <div class="row-label">批量测试并发数（1-8）</div>
        <div class="ctrl">
          <input v-model.number="form.test_concurrency" class="ctrl-input ctrl-input--narrow" type="number" aria-label="批量测试并发数（1-8）" min="1" max="8" />
        </div>
      </div>
    </section>

    <!-- 分组三：托盘菜单 -->
    <section class="group animate-apple-fade-in apple-delay-2">
      <header class="group-head">
        <v-icon icon="mdi-dock-right" size="18" />
        <div class="group-head-text">
          <div class="group-title">托盘菜单</div>
          <div class="group-sub">菜单外观与顶部额度刷新，右键任务栏图标查看效果</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label">任务栏菜单不透明度</div>
        <div class="ctrl slider-wrap">
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
          <span class="slider-value">{{ opacityPercent }}%</span>
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
          <input v-model.number="form.usage_refresh_minutes" class="ctrl-input ctrl-input--narrow" type="number" aria-label="额度刷新间隔（分钟）" min="1" max="1440" />
        </div>
      </div>
    </section>

    <!-- 分组四：关于 -->
    <section class="group animate-apple-fade-in apple-delay-3">
      <header class="group-head">
        <v-icon icon="mdi-information" size="18" />
        <div class="group-head-text">
          <div class="group-title">关于</div>
          <div class="group-sub">当前版本与软件更新，正式版经 GitHub Actions 构建发布并签名校验</div>
        </div>
      </header>
      <div class="row">
        <div class="row-label">当前版本</div>
        <div class="ctrl">
          <span class="about-version">v{{ store.updater.current || "…" }}</span>
        </div>
      </div>
      <div class="row">
        <div class="row-label">软件更新</div>
        <div class="ctrl update-ctrl">
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
        </div>
      </div>
    </section>

    <!-- 页面底部唯一保存 -->
    <div class="save-bar">
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
  background: #fff;
  border: 1px solid #ebeef5;
  border-radius: 8px;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.03);
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
.group-sub {
  margin-top: 2px;
  font-size: 12px;
  line-height: 1.45;
  color: hsl(var(--muted-foreground));
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
  border-top: 1px solid #ebeef5;
}
.row-label {
  font-size: 14px;
  color: hsl(var(--foreground));
  line-height: 1.4;
}

/* 填充式输入：灰底无边框，聚焦白底 + 蓝描边光晕 */
.ctrl {
  flex: none;
}
.ctrl-input {
  width: 320px;
  max-width: 100%;
  height: 34px;
  padding: 0 12px;
  background: #f5f7fa;
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
  color: #a8abb2;
}
.ctrl-input:hover {
  background: #eef1f6;
}
.ctrl-input:focus {
  background: #fff;
  border-color: hsl(var(--accent));
  box-shadow: 0 0 0 3px hsl(var(--accent) / 0.12);
}
.ctrl-input:focus-visible {
  outline: none;
}
.ctrl-input--narrow {
  width: 160px;
}

/* 滑杆：蓝填充轨道 + 白圆钮 */
.slider-wrap {
  display: flex;
  align-items: center;
  gap: 14px;
}
.ctrl-slider {
  appearance: none;
  -webkit-appearance: none;
  width: 220px;
  height: 6px;
  border-radius: 3px;
  background: linear-gradient(to right, hsl(var(--accent)) var(--p), #e4e7ed var(--p));
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
  min-width: 44px;
  text-align: right;
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
  border-top: 1px solid #ebeef5;
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
.update-ctrl {
  display: flex;
  align-items: center;
  gap: 12px;
}
.update-ctrl .v-btn {
  height: 32px;
  min-width: 88px;
  padding: 0 15px;
  border-radius: 6px;
  font-size: 13px;
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
  .ctrl-input--narrow,
  .ctrl-slider {
    width: 100%;
  }
}
</style>
