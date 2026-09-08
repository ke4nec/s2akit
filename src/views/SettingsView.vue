<script setup lang="ts">
import { onBeforeUnmount, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { login, logout, saveConfig, store, submit2fa } from "../store";
import type { AppConfig } from "../types";

function defaultConfig(): AppConfig {
  return {
    base_url: "http://127.0.0.1:8080",
    email: "",
    password: "",
    group_id: null,
    default_model: "",
    test_prompt: "hi",
    test_timeout_secs: 60,
    test_concurrency: 2,
    menu_opacity: 1,
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

/** 托盘菜单不透明度：步进 1 时更新频繁，防抖后再持久化+应用，拖动才不卡 */
let opacityTimer: number | undefined;

function persistMenuOpacity() {
  void invoke("set_menu_opacity", { opacity: form.menu_opacity }).catch(() => {});
}

function onMenuOpacity() {
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
</script>

<template>
  <div class="settings-grid">
    <div class="settings-col">
      <section class="glass-card settings-card animate-apple-fade-in">
        <header class="card-head">
          <v-icon icon="mdi-server" size="18" />
          <div class="card-head-text">
            <div class="card-title">服务与管理员账号</div>
            <div class="card-subtitle">连接 sub2api 服务，凭据仅保存在本机配置文件</div>
          </div>
        </header>
        <div class="card-body">
          <v-text-field
            v-model="form.base_url"
            label="sub2api 服务地址"
            hint="例如 http://127.0.0.1:8080"
            persistent-hint
            density="compact"
          />
          <v-text-field
            v-model="form.email"
            label="管理员邮箱"
            density="compact"
          />
          <v-text-field
            v-model="form.password"
            label="密码"
            type="password"
            density="compact"
          />
          <div class="card-actions">
            <v-btn variant="tonal" :loading="store.saving" @click="doSave">保存</v-btn>
            <v-btn
              color="primary"
              prepend-icon="mdi-login"
              :loading="store.loggingIn"
              @click="doSaveAndLogin"
            >
              保存并登录
            </v-btn>
            <v-btn
              v-if="store.auth"
              variant="text"
              color="error"
              @click="logout()"
            >
              退出登录
            </v-btn>
          </div>
        </div>
      </section>

      <section v-if="store.pending2fa" class="glass-card settings-card animate-apple-scale-in">
        <header class="card-head">
          <v-icon icon="mdi-shield-key-outline" size="18" />
          <div class="card-head-text">
            <div class="card-title">二步验证</div>
            <div class="card-subtitle">输入验证器应用中的 6 位验证码完成登录</div>
          </div>
        </header>
        <div class="card-body">
          <v-text-field
            v-model="totpCode"
            label="6 位验证码"
            maxlength="6"
            density="compact"
            style="max-width: 220px"
          />
          <div class="card-actions">
            <v-btn color="primary" @click="doSubmit2fa">验证并登录</v-btn>
          </div>
        </div>
      </section>
    </div>

    <div class="settings-col">
      <section class="glass-card settings-card animate-apple-fade-in apple-delay-1">
        <header class="card-head">
          <v-icon icon="mdi-speedometer" size="18" />
          <div class="card-head-text">
            <div class="card-title">测速参数</div>
            <div class="card-subtitle">控制账号批量测速的行为与判定</div>
          </div>
        </header>
        <div class="card-body">
          <v-text-field
            v-model="form.default_model"
            label="默认测试模型"
            hint="留空自动选择：优先该账号上次测试的模型，其次平台默认（OpenAI→astra，Claude→opus），否则列表首个文本模型；填写的模型也须在该账号模型列表中才会生效"
            persistent-hint
            density="compact"
          />
          <v-text-field
            v-model="form.test_prompt"
            label="测试 Prompt"
            hint="发给模型的提示词，建议保持极短"
            persistent-hint
            density="compact"
          />
          <div class="num-row">
            <v-text-field
              v-model.number="form.test_timeout_secs"
              type="number"
              label="单次测试超时（秒）"
              density="compact"
            />
            <v-text-field
              v-model.number="form.test_concurrency"
              type="number"
              label="批量测试并发数（1-8）"
              density="compact"
            />
          </div>
          <div class="card-actions">
            <v-btn color="primary" :loading="store.saving" @click="doSave">保存</v-btn>
          </div>
        </div>
      </section>

      <section class="glass-card settings-card animate-apple-fade-in apple-delay-2">
        <header class="card-head">
          <v-icon icon="mdi-palette-outline" size="18" />
          <div class="card-head-text">
            <div class="card-title">外观</div>
            <div class="card-subtitle">调整后立即生效，右键任务栏图标查看效果</div>
          </div>
        </header>
        <div class="card-body">
          <div class="opacity-row">
            <span class="opacity-label">任务栏菜单不透明度</span>
            <span class="opacity-value">{{ Math.round(form.menu_opacity * 100) }}%</span>
          </div>
          <v-slider
            :model-value="form.menu_opacity * 100"
            :min="30"
            :max="100"
            :step="1"
            hide-details
            thumb-label
            @update:model-value="
              (v: number | null) => {
                form.menu_opacity = (v ?? 100) / 100;
                onMenuOpacity();
              }
            "
          />
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
/* 双栏 7:5 网格，窄屏折为单列 */
.settings-grid {
  display: grid;
  grid-template-columns: minmax(0, 7fr) minmax(0, 5fr);
  gap: 16px;
  align-items: start;
}
@media (max-width: 960px) {
  .settings-grid {
    grid-template-columns: 1fr;
  }
}
.settings-col {
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-width: 0;
}
.settings-card {
  padding: 20px 24px 24px;
}
.card-head {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  margin-bottom: 16px;
}
.card-head > .v-icon {
  margin-top: 2px;
  color: hsl(var(--muted-foreground));
}
.card-head-text {
  min-width: 0;
}
.card-title {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
  line-height: 1.3;
  color: hsl(var(--foreground));
}
.card-subtitle {
  margin-top: 2px;
  font-size: 12px;
  line-height: 1.4;
  color: hsl(var(--muted-foreground));
}
.card-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.card-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-top: 4px;
}
.num-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.opacity-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}
.opacity-label {
  font-size: 13px;
  color: hsl(var(--foreground));
}
.opacity-value {
  font-size: 13px;
  font-weight: 600;
  color: hsl(var(--accent));
  font-variant-numeric: tabular-nums;
}
</style>
