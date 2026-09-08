import { createApp } from "vue";
import "vuetify/styles";
import "@mdi/font/css/materialdesignicons.css";
import "./styles/apple.css";
import { createVuetify } from "vuetify";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App.vue";

// Apple-Class 主题：灰白画布 (#F2F3F5) + 白卡片 + 深炭灰文字 + Apple 蓝 (#007AFF) 唯一强调色，
// 语义色取自 Apple 系统色板（绿/橙/红）
const vuetify = createVuetify({
  theme: {
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
    },
  },
  defaults: {
    VTextField: { variant: "outlined" },
    VSelect: { variant: "outlined" },
  },
});

const app = createApp(App);
app.use(vuetify);
app.mount("#app");
// mount 只替换容器内容，class 属性会残留，需手动摘掉启动占位样式
document.getElementById("app")?.classList.remove("boot");

// 主窗口默认隐藏（tauri.conf.json visible:false），首帧渲染完成后再显示，避免白屏；
// 托盘菜单窗口（#tray-menu）的显示由 Rust 侧控制
if (getCurrentWindow().label === "main") {
  void getCurrentWindow()
    .show()
    .then(() => getCurrentWindow().setFocus())
    .catch(() => {
      // 非 Tauri 环境（纯浏览器调试）忽略
    });
}
