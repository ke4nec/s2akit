<script setup lang="ts">
import { computed, onMounted } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  acceptCompliance,
  checkForUpdates,
  dismissUpdate,
  downloadUpdate,
  init,
  installUpdate,
  store,
} from "./store";
import AccountsView from "./views/AccountsView.vue";
import SettingsView from "./views/SettingsView.vue";
import TrayMenuApp from "./views/TrayMenuApp.vue";
import TraySubmenuApp from "./views/TraySubmenuApp.vue";

// 托盘菜单是独立小窗口，加载同一个 SPA 的 #tray-menu 入口；
// 账号级联子菜单是另一个独立小窗口（#tray-submenu），主菜单宽度不变
const isTraySubmenu = window.location.hash.includes("tray-submenu");
const isTrayMenu = !isTraySubmenu && window.location.hash.includes("tray-menu");
// 更新检查只在主窗口进行，托盘小窗口不参与
const isMain = !isTrayMenu && !isTraySubmenu;

const tabs = [
  { value: "accounts", label: "账号", icon: "mdi-format-list-bulleted" },
  { value: "settings", label: "设置", icon: "mdi-cog" },
] as const;

const downloadPercent = computed(() => {
  const { progress, total } = store.updater;
  return total > 0 ? Math.min(100, (progress / total) * 100) : 0;
});

const progressText = computed(() => {
  const { progress, total } = store.updater;
  const fmt = (b: number) => `${(b / 1024 / 1024).toFixed(1)} MB`;
  return total > 0 ? `${fmt(progress)} / ${fmt(total)}` : fmt(progress);
});

onMounted(() => {
  if (!isTrayMenu) void init();
  // 启动 5 秒后静默检查更新；dev 构建跳过，避免开发态误触正式更新
  if (isMain && !import.meta.env.DEV) {
    window.setTimeout(() => void checkForUpdates(), 5000);
  }
});
</script>

<template>
  <TraySubmenuApp v-if="isTraySubmenu" />
  <TrayMenuApp v-else-if="isTrayMenu" />
  <v-app v-else>
    <v-app-bar flat density="compact" class="glass-bar">
      <!-- macOS 分段控件风格导航 -->
      <div class="seg-tabs" role="tablist">
        <button
          v-for="t in tabs"
          :key="t.value"
          type="button"
          role="tab"
          class="seg-tab"
          :class="{ 'seg-tab--active': store.tab === t.value }"
          :aria-selected="store.tab === t.value"
          @click="store.tab = t.value"
        >
          <v-icon :icon="t.icon" size="15" />
          <span>{{ t.label }}</span>
        </button>
      </div>
      <v-spacer />
      <!-- 账号页工具栏传送目标：AccountsView 把分组选择/刷新渲染到此处，与导航合并为一行 -->
      <div class="appbar-tools"></div>
      <div class="auth-status" :title="store.auth?.email">
        <span class="status-dot" :class="store.auth ? 'status-dot--on' : 'status-dot--off'" />
        <span class="auth-email">{{ store.auth ? store.auth.email : "未登录" }}</span>
      </div>
    </v-app-bar>

    <v-main>
      <v-container fluid class="app-container">
        <!-- :key 随标签切换重建节点，重放渐显动画 -->
        <div :key="store.tab" class="animate-apple-fade-in">
          <v-window v-model="store.tab">
            <v-window-item value="accounts">
              <AccountsView />
            </v-window-item>
            <v-window-item value="settings">
              <SettingsView />
            </v-window-item>
          </v-window>
        </div>
      </v-container>
    </v-main>

    <v-snackbar
      v-model="store.snack.show"
      :color="store.snack.color"
      :timeout="3500"
      location="bottom right"
      elevation="6"
    >
      {{ store.snack.message }}
    </v-snackbar>

    <v-dialog :model-value="store.compliance !== null" persistent max-width="560">
      <v-card v-if="store.compliance" rounded="xl" class="compliance-card">
        <div class="compliance-body">
          <div class="compliance-title">管理员合规确认</div>
          <p class="compliance-text">
            该 sub2api 服务要求管理员在首次使用管理接口前确认部署合规承诺：
          </p>
          <div class="compliance-phrase">
            {{ store.compliance.info?.phrase ?? "（正在加载承诺文本…）" }}
          </div>
          <div class="compliance-links">
            <v-btn
              v-if="store.compliance.info?.document_url_zh"
              size="small"
              variant="tonal"
              @click="openUrl(store.compliance.info!.document_url_zh)"
            >
              查看中文文档
            </v-btn>
            <v-btn
              v-if="store.compliance.info?.document_url_en"
              size="small"
              variant="tonal"
              @click="openUrl(store.compliance.info!.document_url_en)"
            >
              English doc
            </v-btn>
          </div>
        </div>
          <div class="compliance-footer">
          <v-btn
            color="primary"
            :disabled="!store.compliance.info"
            @click="acceptCompliance"
          >
            我已阅读并同意
          </v-btn>
        </div>
      </v-card>
    </v-dialog>

    <!-- 应用更新：发现新版本询问 → 后台下载（可收起）→ 校验通过后确认安装重启 -->
    <v-dialog :model-value="store.updater.dialog" persistent max-width="520">
      <v-card v-if="store.updater.dialog" rounded="xl" class="update-card">
        <div class="update-body">
          <div class="update-title">软件更新</div>
          <template v-if="store.updater.status === 'available'">
            <p class="update-text">
              发现新版本 <span class="update-version">v{{ store.updater.version }}</span>（当前 v{{ store.updater.current }}），是否下载更新？
            </p>
            <div v-if="store.updater.notes" class="update-notes">{{ store.updater.notes }}</div>
          </template>
          <div v-else-if="store.updater.status === 'downloading'">
            <p class="update-text">正在下载 v{{ store.updater.version }}…</p>
            <v-progress-linear :model-value="downloadPercent" rounded height="8" color="primary" />
            <p class="update-progress-text">{{ progressText }}</p>
          </div>
          <p v-else-if="store.updater.status === 'ready'" class="update-text">
            v{{ store.updater.version }} 下载完成，完整性校验通过。是否立即安装并重启应用？
          </p>
        </div>
        <div class="update-footer">
          <v-btn
            v-if="store.updater.status === 'downloading'"
            variant="text"
            @click="dismissUpdate"
          >
            后台下载
          </v-btn>
          <v-btn
            v-else-if="store.updater.status === 'available'"
            variant="text"
            @click="dismissUpdate"
          >
            暂不更新
          </v-btn>
          <v-btn
            v-else
            variant="text"
            :disabled="store.updater.installing"
            @click="dismissUpdate"
          >
            稍后
          </v-btn>
          <v-btn
            v-if="store.updater.status === 'available'"
            color="primary"
            @click="downloadUpdate"
          >
            立即更新
          </v-btn>
          <v-btn
            v-else-if="store.updater.status === 'ready'"
            color="primary"
            :loading="store.updater.installing"
            @click="installUpdate"
          >
            立即安装
          </v-btn>
        </div>
      </v-card>
    </v-dialog>
  </v-app>
</template>

<style scoped>
/* macOS 分段控件：灰底容器 + 白色活动段 */
.seg-tabs {
  display: inline-flex;
  gap: 2px;
  margin-left: 24px;
  padding: 2px;
  border-radius: 9px;
  background: rgba(118, 118, 128, 0.12);
}
.seg-tab {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 26px;
  padding: 0 14px;
  border: none;
  border-radius: 7px;
  background: transparent;
  font-family: inherit;
  font-size: 13px;
  font-weight: 400;
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  transition:
    color 0.2s var(--ease-in-out),
    background 0.2s var(--ease-in-out),
    box-shadow 0.2s var(--ease-in-out);
}
.seg-tab:hover {
  color: hsl(var(--foreground));
}
.seg-tab--active {
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  font-weight: 600;
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.1),
    0 0 0 0.5px rgba(0, 0, 0, 0.04);
}

/* 顶栏工具栏区：接收 AccountsView 传送进来的分组选择/测试按钮；
   空态（设置页或未登录）不占视觉空间 */
.appbar-tools {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  margin-right: 12px;
}
.appbar-tools:empty {
  display: none;
  margin-right: 0;
}

/* 登录状态：状态点 + 邮箱，替代原 v-chip */
.auth-status {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-right: 20px;
  max-width: 320px;
  min-width: 0;
}
.status-dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
}
.status-dot--on {
  background: #34c759;
  box-shadow: 0 0 0 3px rgba(52, 199, 89, 0.15);
}
.status-dot--off {
  background: #ff9500;
  box-shadow: 0 0 0 3px rgba(255, 149, 0, 0.15);
}
.auth-email {
  font-size: 13px;
  color: hsl(var(--muted-foreground));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 主内容：1200px 容器，桌面 24px / 窄屏 16px 边距 */
.app-container {
  max-width: 1200px;
  padding: 24px;
}
@media (max-width: 768px) {
  .app-container {
    padding: 16px;
  }
}

/* 合规确认对话框 */
.compliance-card {
  padding: 24px 24px 20px;
}
.compliance-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: hsl(var(--foreground));
}
.compliance-text {
  margin: 10px 0 0;
  font-size: 14px;
  line-height: 1.5;
  color: hsl(var(--muted-foreground));
}
.compliance-phrase {
  margin-top: 12px;
  padding: 12px 14px;
  border-radius: 10px;
  background: hsl(var(--muted));
  font-size: 13px;
  line-height: 1.5;
  color: hsl(var(--foreground));
}
.compliance-links {
  display: flex;
  gap: 8px;
  margin-top: 12px;
}
.compliance-footer {
  display: flex;
  justify-content: flex-end;
  margin-top: 20px;
}

/* 应用更新对话框（沿用合规对话框风格） */
.update-card {
  padding: 24px 24px 20px;
}
.update-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: hsl(var(--foreground));
}
.update-text {
  margin: 10px 0 0;
  font-size: 14px;
  line-height: 1.5;
  color: hsl(var(--muted-foreground));
}
.update-version {
  font-weight: 600;
  color: hsl(var(--accent));
}
.update-notes {
  margin-top: 12px;
  padding: 12px 14px;
  border-radius: 10px;
  background: hsl(var(--muted));
  font-size: 13px;
  line-height: 1.5;
  color: hsl(var(--foreground));
  white-space: pre-line;
  max-height: 180px;
  overflow-y: auto;
}
.update-progress-text {
  margin-top: 8px;
  text-align: right;
  font-size: 12px;
  color: hsl(var(--muted-foreground));
  font-variant-numeric: tabular-nums;
}
.update-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 20px;
}
</style>
