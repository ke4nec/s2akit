import { reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ThemeInstance, ThemeOptions } from "vuetify/lib/composables/theme.js";

/** 主题档位：亮 / 暗 / 跟随系统 */
export type ThemePref = "light" | "dark" | "system";

/**
 * Vuetify 主题配置：Apple-Class 亮 / 暗双色板。
 * 亮色 = 灰白画布 + 白卡片 + Apple 蓝 (#007AFF)；暗色取 Apple 深色系统色
 * (画布 #1A1A1C + 卡片 #242426 + 蓝 #0A84FF + 绿 #30D158 / 橙 #FF9F0A / 红 #FF453A)。
 * 模板里 color="primary" 等语义用法全部走这套，html.dark 类驱动 CSS 变量侧
 */
export const appleThemeOptions: ThemeOptions = {
  defaultTheme: "appleLight",
  themes: {
    appleLight: {
      dark: false,
      colors: {
        background: "#F2F3F5",
        surface: "#FFFFFF",
        "surface-variant": "#F5F5F7",
        "on-surface": "#1D1D1F",
        "on-surface-variant": "#6E6E73",
        primary: "#007AFF",
        "on-primary": "#FFFFFF",
        "primary-darken-1": "#0066D6",
        "primary-lighten-1": "#4DA2FF",
        secondary: "#F5F5F7",
        "on-secondary": "#1D1D1F",
        success: "#34C759",
        "on-success": "#FFFFFF",
        warning: "#FF9500",
        "on-warning": "#FFFFFF",
        error: "#FF3B30",
        "on-error": "#FFFFFF",
        info: "#007AFF",
        "on-info": "#FFFFFF",
        grey: "#8E8E93",
      },
    },
    appleDark: {
      dark: true,
      colors: {
        background: "#1A1A1C",
        surface: "#242426",
        "surface-variant": "#2C2C2E",
        "on-surface": "#F2F2F7",
        "on-surface-variant": "#98989D",
        primary: "#0A84FF",
        "on-primary": "#FFFFFF",
        "primary-darken-1": "#0066D6",
        "primary-lighten-1": "#64A8FF",
        secondary: "#2C2C2E",
        "on-secondary": "#F2F2F7",
        success: "#30D158",
        "on-success": "#0B2B15",
        warning: "#FF9F0A",
        "on-warning": "#2B1B00",
        error: "#FF453A",
        "on-error": "#FFFFFF",
        info: "#0A84FF",
        "on-info": "#FFFFFF",
        grey: "#98989D",
      },
    },
  },
};

/** 主题档位的响应式状态（设置页分段控件绑定用） */
export const themeState = reactive({ pref: "light" as ThemePref });

/** 主题档位展示选项：设置页「外观」组与标题栏快捷菜单共用 */
export const themeOptions: { value: ThemePref; label: string }[] = [
  { value: "light", label: "亮色" },
  { value: "dark", label: "暗色" },
  { value: "system", label: "跟随系统" },
];

/** Vuetify 主题实例：main.ts 装配 createVuetify 后回填（三窗口各一份） */
let vuetifyTheme: ThemeInstance | null = null;

export function bindVuetifyTheme(theme: ThemeInstance) {
  vuetifyTheme = theme;
}

/** index.html 防闪白脚本与这里的读写共用一个 key */
const STORAGE_KEY = "s2akit-theme";

function isPref(v: unknown): v is ThemePref {
  return v === "light" || v === "dark" || v === "system";
}

function systemPrefersDark(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

function resolveDark(pref: ThemePref): boolean {
  return pref === "dark" || (pref === "system" && systemPrefersDark());
}

/**
 * 应用档位到本窗口：html.dark 类 + Vuetify 主题名 + localStorage。
 * report=true 时上报后端（持久化 + 托盘窗口 acrylic + 广播其他窗口），
 * 仅在用户操作或系统明暗变化时使用；收到广播的窗口只应用不回写，避免回声
 */
function applyPref(pref: ThemePref, report: boolean) {
  themeState.pref = pref;
  const dark = resolveDark(pref);
  document.documentElement.classList.toggle("dark", dark);
  if (vuetifyTheme) vuetifyTheme.global.name.value = dark ? "appleDark" : "appleLight";
  try {
    localStorage.setItem(STORAGE_KEY, pref);
  } catch {
    // 隐私模式等场景下 localStorage 不可用，跳过（仅影响下次启动防闪白）
  }
  if (report) {
    void invoke("set_theme", { theme: pref, dark }).catch(() => {
      // 纯浏览器调试时无 Tauri IPC，忽略
    });
  }
}

/** 用户在设置页切换主题：立即生效 + 持久化 + 跨窗口同步 */
export function selectTheme(pref: ThemePref) {
  applyPref(pref, true);
}

/**
 * 窗口启动时的主题初始化（幂等，可被多处安全调用：主窗口走 store.init，
 * 托盘窗口各自 onMounted）：以 config 为权威校正 localStorage（index.html 已按它
 * 提前挂过 dark 类防闪白）、上报实际明暗让后端校正托盘 acrylic（档位未变不写盘
 * 不广播）；监听器（系统明暗 / 跨窗口 theme-changed）只注册一次
 */
let listenersBound = false;

export async function initTheme(configTheme: string) {
  const pref = isPref(configTheme) ? configTheme : "light";
  applyPref(pref, true);
  if (listenersBound) return;
  listenersBound = true;

  window
    .matchMedia?.("(prefers-color-scheme: dark)")
    ?.addEventListener("change", () => {
      if (themeState.pref === "system") applyPref("system", true);
    });

  try {
    await listen<string>("theme-changed", (ev) => {
      if (isPref(ev.payload)) applyPref(ev.payload, false);
    });
  } catch {
    // 纯浏览器调试时无 Tauri 事件总线，忽略监听失败
  }
}
