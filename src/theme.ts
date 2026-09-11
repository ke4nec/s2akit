import { nextTick, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ThemeInstance, ThemeOptions } from "vuetify/lib/composables/theme.js";

/** 主题档位：亮 / 暗 / 跟随系统 */
export type ThemePref = "light" | "dark" | "system";

/**
 * 主题切换动效：圆形扩散 / 淡入淡出 / 擦除滑动 / 模糊渐变 / 平滑同步。
 * 全部基于 View Transitions API（Chromium 111+，WebView2 可用），不支持时
 * 自动回落为平滑同步，保证不出现新旧主题撕裂
 */
export type ThemeFx = "reveal" | "fade" | "wipe" | "blur" | "sync";

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

/** 切换动效的响应式状态（设置页「切换动效」行绑定用） */
export const themeFxState = reactive({ fx: "reveal" as ThemeFx });

/** 切换动效展示选项：设置页「外观」组「切换动效」行共用 */
export const themeFxOptions: { value: ThemeFx; label: string; desc: string }[] = [
  { value: "reveal", label: "圆形扩散", desc: "从点击处圆形扩散，新主题如水波盖住旧主题" },
  { value: "fade", label: "淡入淡出", desc: "全窗口交叉淡化，干净优雅不抢戏" },
  { value: "wipe", label: "擦除滑动", desc: "新主题从左侧如窗帘滑入盖住旧主题" },
  { value: "blur", label: "模糊渐变", desc: "轻微模糊过渡再变清晰，高级感强" },
  { value: "sync", label: "平滑同步", desc: "无花活，全界面颜色同步渐变，修掉先后闪变" },
];

/** Vuetify 主题实例：main.ts 装配 createVuetify 后回填（三窗口各一份） */
let vuetifyTheme: ThemeInstance | null = null;

export function bindVuetifyTheme(theme: ThemeInstance) {
  vuetifyTheme = theme;
}

/** index.html 防闪白脚本与这里的读写共用一个 key */
const STORAGE_KEY = "s2akit-theme";
/** 动效偏好本地缓存 key（与后端 config.theme_fx 双写，启动以 config 为准） */
const STORAGE_FX_KEY = "s2akit-theme-fx";

function isPref(v: unknown): v is ThemePref {
  return v === "light" || v === "dark" || v === "system";
}

function isFx(v: unknown): v is ThemeFx {
  return v === "reveal" || v === "fade" || v === "wipe" || v === "blur" || v === "sync";
}

function systemPrefersDark(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

function resolveDark(pref: ThemePref): boolean {
  return pref === "dark" || (pref === "system" && systemPrefersDark());
}

function prefersReducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

function canViewTransition(): boolean {
  return (
    typeof (document as unknown as Record<string, unknown>).startViewTransition === "function" &&
    !prefersReducedMotion()
  );
}

/** 点击原点：圆形扩散的圆心，无点击来源（如跟随系统自动切）时回落窗口中心 */
export interface ThemeOrigin {
  x: number;
  y: number;
}

function originFromEvent(e: unknown): ThemeOrigin | undefined {
  if (e && typeof e === "object" && "clientX" in e && "clientY" in e) {
    const { clientX, clientY } = e as { clientX: unknown; clientY: unknown };
    if (typeof clientX === "number" && typeof clientY === "number") return { x: clientX, y: clientY };
  }
  return undefined;
}

function centerOrigin(): ThemeOrigin {
  return { x: window.innerWidth / 2, y: window.innerHeight / 2 };
}

/** 覆盖全窗口所需的扩散半径（圆心到四角最大距离 + 16px 余量） */
function coverRadius(o: ThemeOrigin): number {
  const w = window.innerWidth;
  const h = window.innerHeight;
  return Math.hypot(Math.max(o.x, w - o.x), Math.max(o.y, h - o.y)) + 16;
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
  // 经 Vuetify 支持的 change() 切换（与直接写 global.name.value 同效果，
  // 且不触发废弃警告；Vuetify 自带过渡未启用，动效统一走本文件的引擎）
  if (vuetifyTheme) void vuetifyTheme.change(dark ? "appleDark" : "appleLight");
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

/**
 * 平滑同步回落：给 html 挂 theming 类 280ms，期间全界面颜色过渡统一渐变，
 * 画布/卡片/表格同步变化，修掉“先周围变、再表格变”的撕裂；动效不支持
 * View Transitions 或用户选择 sync 时走这里
 */
function applyWithSync(pref: ThemePref, report: boolean) {
  const el = document.documentElement;
  el.classList.add("theming");
  applyPref(pref, report);
  window.setTimeout(() => el.classList.remove("theming"), 280);
}

/** 连续快速点击时跳过动画直接应用，避免过渡队列堆积 */
let fxRunning = false;

/**
 * 带指定动效的主题应用：用户点击走点击原点，系统/广播走窗口中心。
 * View Transitions 不可用或减弱动态偏好时自动回落平滑同步
 */
function applyPrefAnimated(pref: ThemePref, report: boolean, origin?: ThemeOrigin) {
  const fx = themeFxState.fx;
  if (fx === "sync" || !canViewTransition() || fxRunning) {
    applyWithSync(pref, report);
    return;
  }
  const doc = document as unknown as {
    startViewTransition: (cb: () => void | Promise<void>) => { finished: Promise<void> };
  };
  const el = document.documentElement;
  // 收尾幂等：只解冻行内过渡，供 finished/兜底/异常三处复用。
  // data-theme-fx 与圆心变量故意保留到下次切换再覆盖：过渡收尾的合成器
  // 拆卸可能滞后 finished 一两帧，提前摘掉标记会让新旧快照回落到 UA 默认
  // 淡入淡出（旧快照重新显形），即收尾那一下全屏旧模式闪烁；无过渡时这些
  // 规则零作用，残留无害
  const done = () => {
    fxRunning = false;
    el.classList.remove("fx-snap");
  };
  try {
    const o = origin ?? centerOrigin();
    // 快照前冻结行内过渡：开关/按钮/输入框自带的 background 过渡若参与，
    // 新快照只能拍到过渡起点的旧色，快照动画与行内渐变双重叠加即是拖沓感来源；
    // 冻结后快照一次成型，过渡只由伪元素关键帧驱动（样式见 apple.css）
    el.classList.add("fx-snap");
    el.dataset.themeFx = fx;
    if (fx === "reveal") {
      el.style.setProperty("--tx", `${o.x}px`);
      el.style.setProperty("--ty", `${o.y}px`);
      el.style.setProperty("--r", `${coverRadius(o)}px`);
    }
    fxRunning = true;
    // 异步回调：等 Vue 把 Vuetify 主题类、:root 主题变量与界面状态刷进 DOM
    // 后浏览器才抓取新快照，一次拍到终态；否则快照里 Vuetify 部分仍是旧色，
    // 收尾切回真实 DOM 时整片跳变，看起来就是闪一下
    const t = doc.startViewTransition(async () => {
      applyPref(pref, report);
      await nextTick();
    });
    // 无论过渡成功或中止都清理冻结状态；使用 then 的 rejection 分支，
    // 避免 finally 返回的 rejected Promise 在纯浏览器/窗口隐藏场景下未处理
    void t.finished.then(done, done);
    // 兜底：finished 长时间不结算时强制解冻，避免过渡冻结残留
    window.setTimeout(() => {
      if (fxRunning) done();
    }, 1500);
  } catch {
    // 隐藏/最小化窗口等场景下过渡可能抛错，回落直接应用
    done();
    applyWithSync(pref, report);
  }
}

/**
 * 用户切换主题：按当前所选动效播放过渡 + 持久化 + 跨窗口同步。
 * 传点击事件可让圆形扩散从点击处开始，不传则从窗口中心开始
 */
export function selectTheme(pref: ThemePref, ev?: MouseEvent | ThemeOrigin) {
  const origin = ev && "clientX" in ev ? originFromEvent(ev) : ((ev as ThemeOrigin | undefined) ?? undefined);
  applyPrefAnimated(pref, true, origin);
}

/** 用户切换动效偏好：立即生效（下次切主题时用）+ 持久化 + 跨窗口同步 */
export function selectThemeFx(fx: ThemeFx) {
  if (!isFx(fx)) return;
  themeFxState.fx = fx;
  try {
    localStorage.setItem(STORAGE_FX_KEY, fx);
  } catch {
    // localStorage 不可用时跳过，仅影响下次启动前的默认值
  }
  void invoke("set_theme_fx", { themeFx: fx }).catch(() => {
    // 纯浏览器调试时无 Tauri IPC，忽略
  });
}

/** 读本地缓存的动效偏好（启动时 config 到达前的过渡值） */
function readStoredFx(): ThemeFx | null {
  try {
    const v = localStorage.getItem(STORAGE_FX_KEY);
    return isFx(v) ? v : null;
  } catch {
    return null;
  }
}

/**
 * 窗口启动时的主题初始化（幂等，可被多处安全调用：主窗口走 store.init，
 * 托盘窗口各自 onMounted）：以 config 为权威校正 localStorage（index.html 已按它
 * 提前挂过 dark 类防闪白）、上报实际明暗让后端校正托盘 acrylic（档位未变不写盘
 * 不广播）；监听器（系统明暗 / 跨窗口 theme-changed / theme-fx-changed）只注册一次.
 * 启动时直接应用无过渡，避免首帧播放动画
 */
let listenersBound = false;

export async function initTheme(configTheme: string, configFx?: string) {
  const pref = isPref(configTheme) ? configTheme : "light";
  const fx = isFx(configFx) ? configFx : (readStoredFx() ?? "reveal");
  themeFxState.fx = fx;
  try {
    localStorage.setItem(STORAGE_FX_KEY, fx);
  } catch {
    // 忽略，同 selectThemeFx
  }
  applyPref(pref, true);
  if (listenersBound) return;
  listenersBound = true;

  window
    .matchMedia?.("(prefers-color-scheme: dark)")
    ?.addEventListener("change", () => {
      // 跟随系统时的自动切换：无点击来源，从中心播放当前动效
      if (themeState.pref === "system") applyPrefAnimated("system", true);
    });

  try {
    await listen<string>("theme-changed", (ev) => {
      // 其他窗口发起的切换：本窗口跟随播放同款动效（中心原点），只应用不回写
      if (isPref(ev.payload) && ev.payload !== themeState.pref) {
        applyPrefAnimated(ev.payload, false);
      }
    });
    await listen<string>("theme-fx-changed", (ev) => {
      if (isFx(ev.payload)) {
        themeFxState.fx = ev.payload;
        try {
          localStorage.setItem(STORAGE_FX_KEY, ev.payload);
        } catch {
          // 忽略，同上
        }
      }
    });
  } catch {
    // 纯浏览器调试时无 Tauri 事件总线，忽略监听失败
  }
}
