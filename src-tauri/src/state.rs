use crate::config::AppConfig;
use crate::sub2api::{AccountBrief, GroupBrief, KeyBrief, TestResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::RwLock;
use std::time::Instant;

/// 已登录的 token 及其本地计算的过期时间
pub struct AuthEntry {
    pub auth: crate::sub2api::AuthState,
    pub expires_at: Instant,
}

/// 托盘菜单顶部用量缓存：按所选 Key 分键（None = 汇总全部），
/// 在配置的刷新间隔内重复打开菜单不重复请求 API
#[derive(Clone)]
pub struct UsageCache {
    pub key: Option<i64>,
    pub fetched_at: Instant,
    pub usage: Option<crate::commands::KeyUsageToday>,
}

pub struct AppState {
    pub http: reqwest::Client,
    pub config: RwLock<AppConfig>,
    pub config_path: PathBuf,
    pub auth: RwLock<Option<AuthEntry>>,
    /// 当前目标分组名称（托盘显示用）
    pub group_name: RwLock<String>,
    /// 当前目标分组的账号快照（托盘菜单用）
    pub accounts: RwLock<Vec<AccountBrief>>,
    /// API Key 快照（托盘 key 模式首屏高度估算用）
    pub keys: RwLock<Vec<KeyBrief>>,
    pub groups: RwLock<Vec<GroupBrief>>,
    /// 每个账号最近一次测试结果（托盘子菜单悬停展示用）
    pub last_results: RwLock<HashMap<i64, TestResult>>,
    /// 托盘菜单顶部用量缓存（TTL 由配置 usage_refresh_minutes 决定）
    pub usage_cache: RwLock<Option<UsageCache>>,
    /// 顶部用量所选 Key（None = 汇总全部），子菜单“查看额度”切换
    pub usage_key: std::sync::Mutex<Option<i64>>,
    /// 托盘菜单页面是否存活（前端 pong 应答）。
    /// 开发构建加载 vite 服务器，服务器不在时 webview 会停留在错误页，
    /// 借此在下右键时触发重载自愈
    pub menu_alive: AtomicBool,
    /// 托盘菜单锚点：右键时的光标物理坐标，菜单窗口底角定位依据
    pub menu_anchor: std::sync::Mutex<Option<(f64, f64)>>,
    /// 级联子菜单行锚点：账号行相对主菜单卡片上沿的逻辑偏移，子菜单窗口纵向定位依据
    pub submenu_row_top: std::sync::Mutex<Option<f64>>,
}

impl AppState {
    pub fn config_snapshot(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    pub fn base(&self) -> String {
        self.config_snapshot().base()
    }

    pub fn save_config(&self, cfg: &AppConfig) -> std::io::Result<()> {
        cfg.save(&self.config_path)?;
        *self.config.write().unwrap() = cfg.clone();
        Ok(())
    }

    /// token 是否有效（预留 60 秒提前量）
    pub fn valid_token(&self) -> Option<String> {
        let guard = self.auth.read().unwrap();
        guard
            .as_ref()
            .filter(|e| Instant::now() + std::time::Duration::from_secs(60) < e.expires_at)
            .map(|e| e.auth.access_token.clone())
    }
}
