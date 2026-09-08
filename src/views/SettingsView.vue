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
  <v-row>
    <v-col cols="12" md="7">
      <v-card elevation="0" :border="true">
        <v-card-title class="text-subtitle-1">服务与管理员账号</v-card-title>
        <v-card-text>
          <v-text-field
            v-model="form.base_url"
            label="sub2api 服务地址"
            hint="例如 http://127.0.0.1:8080"
            persistent-hint
            density="compact"
            class="mb-2"
          />
          <v-text-field
            v-model="form.email"
            label="管理员邮箱"
            density="compact"
            class="mb-2"
          />
          <v-text-field
            v-model="form.password"
            label="密码"
            type="password"
            density="compact"
            hint="凭据保存在本机配置文件中"
            persistent-hint
          />
          <div class="d-flex ga-2 mt-4 flex-wrap">
            <v-btn size="small" variant="text" :loading="store.saving" @click="doSave">
              保存
            </v-btn>
            <v-btn
              size="small"
              color="primary"
              :loading="store.loggingIn"
              @click="doSaveAndLogin"
            >
              保存并登录
            </v-btn>
            <v-btn
              v-if="store.auth"
              size="small"
              variant="text"
              color="error"
              @click="logout()"
            >
              退出登录
            </v-btn>
          </div>
        </v-card-text>
      </v-card>

      <v-card v-if="store.pending2fa" elevation="0" :border="true" class="mt-4">
        <v-card-title class="text-subtitle-1">二步验证</v-card-title>
        <v-card-text>
          <v-text-field
            v-model="totpCode"
            label="6 位验证码"
            maxlength="6"
            density="compact"
            style="max-width: 220px"
          />
          <v-btn size="small" color="primary" @click="doSubmit2fa">验证并登录</v-btn>
        </v-card-text>
      </v-card>
    </v-col>

    <v-col cols="12" md="5">
      <v-card elevation="0" :border="true">
        <v-card-title class="text-subtitle-1">测速参数</v-card-title>
        <v-card-text>
          <v-text-field
            v-model="form.default_model"
            label="默认测试模型"
            hint="留空自动选择：优先该账号上次测试的模型，其次平台默认（OpenAI→astra，Claude→opus），否则列表首个文本模型；填写的模型也须在该账号模型列表中才会生效"
            persistent-hint
            density="compact"
            class="mb-2"
          />
          <v-text-field
            v-model="form.test_prompt"
            label="测试 Prompt"
            hint="发给模型的提示词，建议保持极短"
            persistent-hint
            density="compact"
            class="mb-2"
          />
          <v-text-field
            v-model.number="form.test_timeout_secs"
            type="number"
            label="单次测试超时（秒）"
            density="compact"
            class="mb-2"
          />
          <v-text-field
            v-model.number="form.test_concurrency"
            type="number"
            label="批量测试并发数（1-8）"
            density="compact"
          />
          <v-btn size="small" color="primary" :loading="store.saving" @click="doSave">
            保存
          </v-btn>
        </v-card-text>
      </v-card>

      <v-card elevation="0" :border="true" class="mt-4">
        <v-card-title class="text-subtitle-1">外观</v-card-title>
        <v-card-text>
          <div class="text-caption text-medium-emphasis mb-1">
            任务栏菜单不透明度：{{ Math.round(form.menu_opacity * 100) }}%
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
          <div class="text-caption text-disabled mt-1">调整后立即生效，右键任务栏图标查看效果</div>
        </v-card-text>
      </v-card>
    </v-col>
  </v-row>
</template>
