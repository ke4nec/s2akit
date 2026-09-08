use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub base_url: String,
    pub email: String,
    pub password: String,
    pub group_id: Option<i64>,
    /// 留空表示自动选择测试模型（上次模型 → 平台默认 → 列表首个文本模型）
    pub default_model: String,
    pub test_prompt: String,
    pub test_timeout_secs: u64,
    pub test_concurrency: usize,
    /// 托盘菜单窗口不透明度 0.3~1.0
    pub menu_opacity: f32,
    /// 托盘菜单顶部额度信息的缓存时长（分钟），有效范围 1~1440
    pub usage_refresh_minutes: u64,
    /// 每个账号上次测试实际使用的模型（account_id -> model_id），
    /// 作为该账号下次测试的默认模型
    pub last_models: HashMap<i64, String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8080".into(),
            email: String::new(),
            password: String::new(),
            group_id: None,
            default_model: String::new(),
            test_prompt: "hi".into(),
            test_timeout_secs: 60,
            test_concurrency: 2,
            menu_opacity: 1.0,
            usage_refresh_minutes: Self::DEFAULT_USAGE_REFRESH_MINUTES,
            last_models: HashMap::new(),
        }
    }
}

impl AppConfig {
    /// 用量缓存时长的默认值与上下限（分钟）：默认 10 分钟，最短 1 分钟，最长 24 小时
    pub const DEFAULT_USAGE_REFRESH_MINUTES: u64 = 10;
    pub const MIN_USAGE_REFRESH_MINUTES: u64 = 1;
    pub const MAX_USAGE_REFRESH_MINUTES: u64 = 24 * 60;

    /// 夹取到有效范围的缓存时长，防止手改配置文件写入越界值
    pub fn usage_refresh_minutes_clamped(&self) -> u64 {
        self.usage_refresh_minutes
            .clamp(Self::MIN_USAGE_REFRESH_MINUTES, Self::MAX_USAGE_REFRESH_MINUTES)
    }

    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }

    /// 去掉尾部斜杠的服务地址
    pub fn base(&self) -> String {
        self.base_url.trim().trim_end_matches('/').to_string()
    }
}
