<script setup lang="ts">
import { onMounted } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { acceptCompliance, init, store } from "./store";
import AccountsView from "./views/AccountsView.vue";
import SettingsView from "./views/SettingsView.vue";
import TrayMenuApp from "./views/TrayMenuApp.vue";

// 托盘菜单是独立小窗口，加载同一个 SPA 的 #tray-menu 入口
const isTrayMenu = window.location.hash.includes("tray-menu");

onMounted(() => {
  if (!isTrayMenu) void init();
});
</script>

<template>
  <TrayMenuApp v-if="isTrayMenu" />
  <v-app v-else>
    <v-app-bar flat color="surface" :border="true" density="compact">
      <v-icon icon="mdi-swap-horizontal-circle" color="primary" class="ml-4" />
      <span class="text-subtitle-1 font-weight-bold ml-2">s2akit</span>
      <v-divider vertical inset class="mx-4" />
      <v-tabs v-model="store.tab" density="compact" color="primary">
        <v-tab value="accounts" prepend-icon="mdi-format-list-bulleted">账号</v-tab>
        <v-tab value="settings" prepend-icon="mdi-cog">设置</v-tab>
      </v-tabs>
      <v-spacer />
      <v-chip
        v-if="store.auth"
        size="small"
        color="success"
        variant="tonal"
        prepend-icon="mdi-check-circle"
        class="mr-4"
      >
        {{ store.auth.email }}
      </v-chip>
      <v-chip v-else size="small" color="warning" variant="tonal" prepend-icon="mdi-alert" class="mr-4">
        未登录
      </v-chip>
    </v-app-bar>

    <v-main>
      <v-container fluid class="pa-4">
        <v-window v-model="store.tab">
          <v-window-item value="accounts">
            <AccountsView />
          </v-window-item>
          <v-window-item value="settings">
            <SettingsView />
          </v-window-item>
        </v-window>
      </v-container>
    </v-main>

    <v-snackbar
      v-model="store.snack.show"
      :color="store.snack.color"
      :timeout="3500"
      location="bottom right"
    >
      {{ store.snack.message }}
    </v-snackbar>

    <v-dialog :model-value="store.compliance !== null" persistent max-width="560">
      <v-card v-if="store.compliance">
        <v-card-title class="text-h6">管理员合规确认</v-card-title>
        <v-card-text class="text-body-2">
          <p>该 sub2api 服务要求管理员在首次使用管理接口前确认部署合规承诺：</p>
          <p class="mt-3 pa-3 rounded bg-grey-lighten-4 text-medium-emphasis">
            {{ store.compliance.info?.phrase ?? "（正在加载承诺文本…）" }}
          </p>
          <div class="d-flex ga-2 mt-3">
            <v-btn
              v-if="store.compliance.info?.document_url_zh"
              size="small"
              variant="outlined"
              @click="openUrl(store.compliance.info!.document_url_zh)"
            >
              查看中文文档
            </v-btn>
            <v-btn
              v-if="store.compliance.info?.document_url_en"
              size="small"
              variant="outlined"
              @click="openUrl(store.compliance.info!.document_url_en)"
            >
              English doc
            </v-btn>
          </div>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn color="primary" :disabled="!store.compliance.info" @click="acceptCompliance">
            我已阅读并同意
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </v-app>
</template>

<style>
html {
  overflow-y: auto;
}
/* 全局统一滚动条：细条、圆角、悬停加深（覆盖 v-data-table 内部滚动区域） */
*::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}
*::-webkit-scrollbar-track {
  background: transparent;
}
*::-webkit-scrollbar-thumb {
  background: rgba(0, 0, 0, 0.22);
  border-radius: 4px;
}
*::-webkit-scrollbar-thumb:hover {
  background: rgba(0, 0, 0, 0.38);
}
* {
  scrollbar-width: thin;
  scrollbar-color: rgba(0, 0, 0, 0.22) transparent;
}
</style>
