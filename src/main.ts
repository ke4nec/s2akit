import { createApp } from "vue";
import "vuetify/styles";
import "@mdi/font/css/materialdesignicons.css";
import "./styles/apple.css";
import "./styles/tables.css";
import { createVuetify } from "vuetify";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App.vue";
import { appleThemeOptions, bindVuetifyTheme } from "./theme";

// Apple-Class 双主题：亮 = 灰白画布 (#F2F3F5) + 白卡片 + Apple 蓝 (#007AFF)；
// 暗 = Apple 深色系统色（画布 #1A1A1C + 卡片 #242426 + 蓝 #0A84FF）。
// 主题配置与切换逻辑集中在 theme.ts，装配后把实例回填给它
const vuetify = createVuetify({
  theme: appleThemeOptions,
  defaults: {
    VTextField: { variant: "outlined" },
    VSelect: { variant: "outlined" },
  },
});
bindVuetifyTheme(vuetify.theme);

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
