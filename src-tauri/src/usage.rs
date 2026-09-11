//! 顶部用量统计：所选 Key 或汇总全部 Key（托盘菜单顶部 / 主窗口同源）。
//! 由 commands.rs 按域拆出；跨命令共享的认证闭包走 `crate::commands::authed`。

use crate::commands::authed;
use crate::error::AppResult;
use crate::state::AppState;
use crate::sub2api as api;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, serde::Serialize)]
pub struct KeyUsageToday {
    pub key_id: i64,
    pub key_name: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_tokens: i64,
    pub total_tokens: i64,
    pub cost: f64,
}

/// 顶部用量展示：所选 Key（子菜单“查看额度”切换）或汇总全部 Key。
/// 结果按所选 key 分键缓存（usage_refresh_minutes，默认 10 分钟），
/// 间隔内重复打开直接返回缓存；force=true 绕过缓存强制刷新。
#[tauri::command]
pub async fn get_usage_today(app: AppHandle, force: Option<bool>) -> AppResult<Option<KeyUsageToday>> {
    let state = app.state::<AppState>();
    let ttl = Duration::from_secs(state.config_snapshot().usage_refresh_minutes_clamped() * 60);
    let sel = *state.usage_key.lock().unwrap_or_else(|e| e.into_inner());
    if !force.unwrap_or(false) {
        if let Some(cache) = state.usage_cache.read().unwrap().as_ref() {
            if cache.key == sel && cache.fetched_at.elapsed() < ttl {
                return Ok(cache.usage.clone());
            }
        }
    }
    let (http, base) = (state.http.clone(), state.config_snapshot().base());
    drop(state);

    let usage = match fetch_usage_selection(&app, http, base, sel).await {
        Ok(usage) => usage,
        // 拉取失败时退回同选择旧缓存（宁可显示过期数据也不闪空），无缓存才报错
        Err(e) => {
            return match app.state::<AppState>().usage_cache.read().unwrap().clone() {
                Some(cache) if cache.key == sel => Ok(cache.usage),
                _ => Err(e),
            }
        }
    };
    cache_usage(&app, sel, usage.clone());
    Ok(usage)
}

/// 切换顶部用量所选 Key（None = 汇总全部），子菜单“查看额度”调用
#[tauri::command]
pub fn set_usage_key(app: AppHandle, key_id: Option<i64>) {
    *app.state::<AppState>()
        .usage_key
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = key_id;
}

/// 查询当前所选（子菜单勾选态用）
#[tauri::command]
pub fn get_usage_key(app: AppHandle) -> Option<i64> {
    *app.state::<AppState>()
        .usage_key
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 所选 Key 的当天用量；None = 汇总全部 Key（逐个拉取后相加，失败的单 key 跳过）。
/// 所选 key 已不在列表时回落汇总并复位选择，避免用量区永久消失。
async fn fetch_usage_selection(
    app: &AppHandle,
    http: reqwest::Client,
    base: String,
    sel: Option<i64>,
) -> AppResult<Option<KeyUsageToday>> {
    let keys = {
        let http = http.clone();
        let base = base.clone();
        authed(app, move |token| {
            let http = http.clone();
            let base = base.clone();
            async move { api::list_keys(&http, &base, &token).await }
        })
        .await?
    };
    if keys.is_empty() {
        return Ok(None);
    }
    let sel = match sel {
        Some(id) if keys.iter().any(|k| k.id == id) => Some(id),
        Some(_) => {
            *app.state::<AppState>()
                .usage_key
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            None
        }
        None => None,
    };
    if let Some(id) = sel {
        // 上方 any() 已确认存在，find 必中
        let key = keys.iter().find(|k| k.id == id).unwrap();
        let stats = {
            let http = http.clone();
            let base = base.clone();
            authed(app, move |token| {
                let http = http.clone();
                let base = base.clone();
                async move { api::key_usage_today(&http, &base, &token, id).await }
            })
            .await?
        };
        return Ok(Some(KeyUsageToday {
            key_id: key.id,
            key_name: key.name.clone(),
            requests: stats.total_requests,
            input_tokens: stats.total_input_tokens,
            output_tokens: stats.total_output_tokens,
            cache_tokens: stats.total_cache_tokens,
            total_tokens: stats.total_tokens,
            cost: stats.total_cost,
        }));
    }
    let mut requests = 0i64;
    let mut input_tokens = 0i64;
    let mut output_tokens = 0i64;
    let mut cache_tokens = 0i64;
    let mut total_tokens = 0i64;
    let mut cost = 0f64;
    let mut n = 0usize;
    for k in &keys {
        let http = http.clone();
        let base = base.clone();
        let kid = k.id;
        let stats = authed(app, move |token| {
            let http = http.clone();
            let base = base.clone();
            async move { api::key_usage_today(&http, &base, &token, kid).await }
        })
        .await;
        let Ok(stats) = stats else {
            continue;
        };
        n += 1;
        requests += stats.total_requests;
        input_tokens += stats.total_input_tokens;
        output_tokens += stats.total_output_tokens;
        cache_tokens += stats.total_cache_tokens;
        total_tokens += stats.total_tokens;
        cost += stats.total_cost;
    }
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(KeyUsageToday {
        key_id: 0,
        key_name: format!("All Keys ({n})"),
        requests,
        input_tokens,
        output_tokens,
        cache_tokens,
        total_tokens,
        cost,
    }))
}

/// Key 页用量列：全部 Key 各自的当天用量（拉取失败的单 key 跳过，不缓存）
#[tauri::command]
pub async fn list_keys_usage(app: AppHandle) -> AppResult<Vec<KeyUsageToday>> {
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let base = state.config_snapshot().base();
    drop(state);
    let keys = {
        let http = http.clone();
        let base = base.clone();
        authed(&app, move |token| {
            let http = http.clone();
            let base = base.clone();
            async move { api::list_keys(&http, &base, &token).await }
        })
        .await?
    };
    let mut out = Vec::with_capacity(keys.len());
    for k in &keys {
        let http = http.clone();
        let base = base.clone();
        let kid = k.id;
        let stats = authed(&app, move |token| {
            let http = http.clone();
            let base = base.clone();
            async move { api::key_usage_today(&http, &base, &token, kid).await }
        })
        .await;
        let Ok(stats) = stats else {
            continue;
        };
        out.push(KeyUsageToday {
            key_id: k.id,
            key_name: k.name.clone(),
            requests: stats.total_requests,
            input_tokens: stats.total_input_tokens,
            output_tokens: stats.total_output_tokens,
            cache_tokens: stats.total_cache_tokens,
            total_tokens: stats.total_tokens,
            cost: stats.total_cost,
        });
    }
    Ok(out)
}

/// 记录/覆盖用量缓存（按所选 key 分键；无用量记录的 None 也缓存，同样受间隔约束）
fn cache_usage(app: &AppHandle, key: Option<i64>, usage: Option<KeyUsageToday>) {
    *app.state::<AppState>().usage_cache.write().unwrap() = Some(crate::state::UsageCache {
        key,
        fetched_at: Instant::now(),
        usage,
    });
}
