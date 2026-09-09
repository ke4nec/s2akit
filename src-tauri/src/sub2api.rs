//! sub2api 管理 API 客户端。
//! 契约已于 2026-09-07 对本机 127.0.0.1:8080 实例实测确认：
//! - 统一 envelope `{code: 0|大写字符串, message, data}`，成功时 code 为数字 0
//! - 账号列表 data 为 `{items, total, page, page_size, pages}`
//! - 测试接口为 SSE：仅 `data: <单行JSON>\n\n`，无 `event:` 行，按 type 字段路由

use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

// ---------- 数据结构 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub email: String,
    #[serde(default)]
    pub username: String,
    /// admin / user：决定前端展示分组与账号管理入口
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    pub user: UserInfo,
}

pub enum LoginOutcome {
    Ok(AuthState),
    TwoFactorRequired { temp_token: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupBrief {
    pub id: i64,
    pub name: String,
    pub platform: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub account_count: Option<i64>,
    #[serde(default)]
    pub active_account_count: Option<i64>,
}

fn default_status() -> String {
    "active".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBrief {
    pub id: i64,
    pub name: String,
    pub platform: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub status: String,
    pub schedulable: bool,
    #[serde(default)]
    pub error_message: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub rate_limited: bool,
    #[serde(default)]
    pub temp_unschedulable: bool,
    #[serde(default)]
    pub group_ids: Vec<i64>,
}

/// 服务端原始账号对象（含派生字段源），转换为 AccountBrief
#[derive(Debug, Deserialize)]
struct RawAccount {
    id: i64,
    name: String,
    platform: String,
    #[serde(rename = "type")]
    account_type: String,
    #[serde(default = "default_status")]
    status: String,
    #[serde(default)]
    schedulable: bool,
    #[serde(default)]
    error_message: String,
    #[serde(default)]
    priority: i64,
    #[serde(default)]
    rate_limited_at: Option<Value>,
    #[serde(default)]
    overload_until: Option<Value>,
    #[serde(default)]
    temp_unschedulable_until: Option<Value>,
    #[serde(default)]
    group_ids: Vec<i64>,
}

impl From<RawAccount> for AccountBrief {
    fn from(r: RawAccount) -> Self {
        AccountBrief {
            id: r.id,
            name: r.name,
            platform: r.platform,
            account_type: r.account_type,
            status: r.status,
            schedulable: r.schedulable,
            error_message: r.error_message,
            priority: r.priority,
            rate_limited: r.rate_limited_at.as_ref().map(is_set).unwrap_or(false),
            temp_unschedulable: r.overload_until.as_ref().map(is_set).unwrap_or(false)
                || r.temp_unschedulable_until.as_ref().map(is_set).unwrap_or(false),
            group_ids: r.group_ids,
        }
    }
}

/// 字段为 null 或空串时视为未设置
fn is_set(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelBrief {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceInfo {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub phrase: String,
    #[serde(default)]
    pub document_url_zh: String,
    #[serde(default)]
    pub document_url_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub account_id: i64,
    #[serde(default)]
    pub account_name: String,
    pub success: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    #[serde(default)]
    pub total_ms: Option<u64>,
    #[serde(default)]
    pub content_preview: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// 发给前端（Channel 与事件共用）的测试进度消息
#[derive(Debug, Clone, Serialize)]
pub struct TestProgress {
    pub account_id: i64,
    /// start | content | result
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TestResult>,
}

// ---------- envelope 解析 ----------

/// 解析 `{code, message, data}` envelope。code 为 0 时反序列化 data，
/// 否则携带 code/HTTP status 返回错误（保留 code 供 423 合规、401 重试判断）。
async fn envelope<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> AppResult<T> {
    let status = resp.status().as_u16();
    let value: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return Err(AppError::Api {
                message: format!("HTTP {status}: 响应不是有效 JSON（{e}）"),
                code: None,
                status: Some(status),
            })
        }
    };
    let code = &value["code"];
    if code.is_number() && code.as_i64() == Some(0) {
        serde_json::from_value(value["data"].clone())
            .map_err(|e| AppError::Other(format!("响应解析失败: {e}")))
    } else {
        let code_str = code
            .as_str()
            .map(str::to_string)
            .or_else(|| code.as_i64().map(|n| n.to_string()));
        let message = value["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| "未知错误".into());
        Err(AppError::Api {
            message,
            code: code_str,
            status: Some(status),
        })
    }
}

/// 与 envelope 相同但只要确认成功（data 内容不关心）
async fn envelope_ok(resp: reqwest::Response) -> AppResult<()> {
    envelope::<Value>(resp).await.map(|_| ())
}

// ---------- 认证 ----------

fn parse_auth(data: &Value) -> AppResult<AuthState> {
    serde_json::from_value(data.clone()).map_err(|e| AppError::Other(format!("登录响应解析失败: {e}")))
}

pub async fn login(
    http: &reqwest::Client,
    base: &str,
    email: &str,
    password: &str,
) -> AppResult<LoginOutcome> {
    let resp = http
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({ "email": email, "password": password }))
        .send()
        .await?;
    let status = resp.status().as_u16();
    let value: Value = resp.json().await.map_err(|e| {
        AppError::Api {
            message: format!("HTTP {status}: 登录响应异常（{e}）"),
            code: None,
            status: Some(status),
        }
    })?;
    let code = &value["code"];
    if !(code.is_number() && code.as_i64() == Some(0)) {
        let code_str = code
            .as_str()
            .map(str::to_string)
            .or_else(|| code.as_i64().map(|n| n.to_string()));
        let message = value["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| "登录失败".into());
        return Err(AppError::Api {
            message,
            code: code_str,
            status: Some(status),
        });
    }
    let data = &value["data"];
    if data["requires_2fa"].as_bool() == Some(true) {
        return Ok(LoginOutcome::TwoFactorRequired {
            temp_token: data["temp_token"].as_str().unwrap_or_default().to_string(),
        });
    }
    Ok(LoginOutcome::Ok(parse_auth(data)?))
}

pub async fn login_2fa(
    http: &reqwest::Client,
    base: &str,
    temp_token: &str,
    code: &str,
) -> AppResult<AuthState> {
    let resp = http
        .post(format!("{base}/api/v1/auth/login/2fa"))
        .json(&json!({ "temp_token": temp_token, "code": code }))
        .send()
        .await?;
    envelope(resp).await
}

pub async fn refresh(http: &reqwest::Client, base: &str, refresh_token: &str) -> AppResult<AuthState> {
    let resp = http
        .post(format!("{base}/api/v1/auth/refresh"))
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await?;
    envelope(resp).await
}

// ---------- 合规确认 ----------

pub async fn get_compliance(
    http: &reqwest::Client,
    base: &str,
    token: &str,
) -> AppResult<ComplianceInfo> {
    let resp = http
        .get(format!("{base}/api/v1/admin/compliance"))
        .bearer_auth(token)
        .send()
        .await?;
    let v: Value = envelope(resp).await?;
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    Ok(ComplianceInfo {
        version: s("version"),
        phrase: s("phrase"),
        document_url_zh: s("document_url_zh"),
        document_url_en: s("document_url_en"),
    })
}

pub async fn accept_compliance(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    phrase: &str,
) -> AppResult<()> {
    let language = if phrase.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
        "zh"
    } else {
        "en"
    };
    let resp = http
        .post(format!("{base}/api/v1/admin/compliance/accept"))
        .bearer_auth(token)
        .json(&json!({ "phrase": phrase, "language": language }))
        .send()
        .await?;
    envelope_ok(resp).await
}

// ---------- 分组 / 账号 / 模型 / 调度 ----------

pub async fn list_groups(http: &reqwest::Client, base: &str, token: &str) -> AppResult<Vec<GroupBrief>> {
    let resp = http
        .get(format!("{base}/api/v1/admin/groups/all"))
        .bearer_auth(token)
        .send()
        .await?;
    envelope(resp).await
}

/// 按分组取账号（group 为 None 时取全部），自动翻页
pub async fn list_accounts(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    group: Option<i64>,
) -> AppResult<Vec<AccountBrief>> {
    let mut all: Vec<AccountBrief> = Vec::new();
    let mut page = 1u32;
    loop {
        let mut url = format!("{base}/api/v1/admin/accounts?page={page}&page_size=200");
        if let Some(g) = group {
            url.push_str(&format!("&group={g}"));
        }
        let resp = http.get(url).bearer_auth(token).send().await?;
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            items: Vec<RawAccount>,
            #[serde(default)]
            pages: Option<u32>,
        }
        let p: Page = envelope(resp).await?;
        let got = p.items.len();
        all.extend(p.items.into_iter().map(AccountBrief::from));
        let pages = p.pages.unwrap_or(1).max(1);
        if got == 0 || page >= pages || page >= 10 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

pub async fn account_models(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    account_id: i64,
) -> AppResult<Vec<ModelBrief>> {
    let resp = http
        .get(format!("{base}/api/v1/admin/accounts/{account_id}/models"))
        .bearer_auth(token)
        .send()
        .await?;
    envelope(resp).await
}

pub async fn set_schedulable(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    account_id: i64,
    schedulable: bool,
) -> AppResult<()> {
    let resp = http
        .post(format!("{base}/api/v1/admin/accounts/{account_id}/schedulable"))
        .bearer_auth(token)
        .json(&json!({ "schedulable": schedulable }))
        .send()
        .await?;
    envelope_ok(resp).await
}

// ---------- API Key 列表 / 状态 / 模型 / 测试 ----------

/// 托盘 key 模式用：列表接口直接返回明文 secret，前端测试 /models 与对话时用它做 bearer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBrief {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub key: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub group_id: Option<i64>,
}

/// key 列表（自动翻页，最多 10 页）
pub async fn list_keys(http: &reqwest::Client, base: &str, token: &str) -> AppResult<Vec<KeyBrief>> {
    let mut all: Vec<KeyBrief> = Vec::new();
    let mut page = 1u32;
    loop {
        let resp = http
            .get(format!("{base}/api/v1/keys?page={page}&pageSize=200"))
            .bearer_auth(token)
            .send()
            .await?;
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            items: Vec<KeyBrief>,
            #[serde(default)]
            pages: Option<u32>,
        }
        let p: Page = envelope(resp).await?;
        let got = p.items.len();
        all.extend(p.items);
        let pages = p.pages.unwrap_or(1).max(1);
        if got == 0 || page >= pages || page >= 10 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

/// key 启停（status 取 active / inactive，服务端鉴权：只能操作自己的 key）
pub async fn set_key_status(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    id: i64,
    status: &str,
) -> AppResult<()> {
    let resp = http
        .put(format!("{base}/api/v1/keys/{id}"))
        .bearer_auth(token)
        .json(&json!({ "status": status }))
        .send()
        .await?;
    envelope_ok(resp).await
}

/// 用 key 自身做 bearer 取 OpenAI 兼容模型列表（无 envelope 包裹）
pub async fn key_models(
    http: &reqwest::Client,
    base: &str,
    key_secret: &str,
) -> AppResult<Vec<ModelBrief>> {
    let resp = http
        .get(format!("{base}/v1/models"))
        .bearer_auth(key_secret)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(openai_err(resp).await);
    }
    #[derive(Deserialize)]
    struct List {
        #[serde(default)]
        data: Vec<ModelBrief>,
    }
    resp.json()
        .await
        .map(|l: List| l.data)
        .map_err(|e| AppError::Other(format!("模型列表解析失败: {e}")))
}

/// OpenAI 错误体解析：{message} 或 {error:{message}}，兜底 HTTP 状态
async fn openai_err(resp: reqwest::Response) -> AppError {
    let status = resp.status().as_u16();
    match resp.json::<Value>().await {
        Ok(v) => {
            let msg = v["message"]
                .as_str()
                .or_else(|| v["error"]["message"].as_str())
                .unwrap_or("未知错误");
            AppError::Api {
                message: msg.to_string(),
                code: None,
                status: Some(status),
            }
        }
        Err(_) => AppError::Api {
            message: format!("HTTP {status}"),
            code: None,
            status: Some(status),
        },
    }
}

/// 媒体模型判定（key 自动选模型时跳过，与账号侧规则一致）
fn is_media_model_id(id: &str) -> bool {
    let id = id.to_lowercase();
    ["image", "video", "audio", "tts", "whisper", "sora", "realtime", "dall", "veo"]
        .iter()
        .any(|k| id.contains(k))
}

/// 用 key 直接做 OpenAI 兼容对话测试（SSE），计时口径与账号测试一致：
/// 首个非空 content 增量 = 首 token；finish / [DONE] = 总耗时
#[allow(clippy::too_many_arguments)]
pub async fn test_key(
    http: &reqwest::Client,
    base: &str,
    key_secret: &str,
    key_id: i64,
    key_name: &str,
    model: Option<&str>,
    prompt: &str,
    timeout: Duration,
) -> TestResult {
    let start = Instant::now();
    let mut result = TestResult {
        // 复用字段承载 key 维度（前端按 key 展示，不与账号结果混存）
        account_id: key_id,
        account_name: key_name.to_string(),
        success: false,
        model: model.unwrap_or("").to_string(),
        first_token_ms: None,
        total_ms: None,
        content_preview: String::new(),
        error: None,
    };
    // 未指定模型时取 /v1/models 首个非媒体模型（与“自动选择”语义一致）
    let model = match model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()) {
        Some(m) => Some(m),
        None => match key_models(http, base, key_secret).await {
            Ok(models) => models
                .into_iter()
                .map(|m| m.id)
                .find(|id| !is_media_model_id(id)),
            Err(e) => {
                finish_err(&mut result, &start, &format!("获取模型列表失败: {e}"));
                return result;
            }
        },
    };
    let Some(model) = model else {
        finish_err(&mut result, &start, "该 Key 没有可用模型");
        return result;
    };
    result.model = model.clone();

    let send = async {
        http.post(format!("{base}/v1/chat/completions"))
            .bearer_auth(key_secret)
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": prompt}],
                "stream": true,
            }))
            .send()
            .await
    };

    let resp = match tokio::time::timeout(Duration::from_secs(20), send).await {
        Err(_) => {
            finish_err(&mut result, &start, "连接超时（20 秒无响应）");
            return result;
        }
        Ok(Err(e)) => {
            finish_err(&mut result, &start, &format!("网络错误: {e}"));
            return result;
        }
        Ok(Ok(r)) => r,
    };

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let msg = match resp.json::<Value>().await {
            Ok(v) => v["message"]
                .as_str()
                .or_else(|| v["error"]["message"].as_str())
                .unwrap_or("未知错误")
                .to_string(),
            Err(_) => format!("HTTP {status}"),
        };
        finish_err(&mut result, &start, &msg);
        return result;
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let deadline = start + timeout;
    let mut terminal = false;

    while !terminal {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            finish_err(&mut result, &start, &format!("测试超时（{} 秒）", timeout.as_secs()));
            break;
        }
        match tokio::time::timeout(remain, stream.next()).await {
            Err(_) => {
                if !result.success {
                    finish_err(&mut result, &start, &format!("测试超时（{} 秒）", timeout.as_secs()));
                }
                break;
            }
            Ok(None) => {
                if !result.success && result.error.is_none() {
                    // 流正常结束：按成功结算（与 [DONE] 同语义）
                    result.success = true;
                    result.total_ms = Some(start.elapsed().as_millis() as u64);
                }
                break;
            }
            Ok(Some(Err(e))) => {
                if !result.success && result.error.is_none() {
                    finish_err(&mut result, &start, &format!("网络错误: {e}"));
                }
                break;
            }
            Ok(Some(Ok(chunk))) => {
                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                    let frame: Vec<u8> = buf.drain(..pos + 2).collect();
                    let frame_text = String::from_utf8_lossy(&frame);
                    for line in frame_text.lines() {
                        let Some(payload) = line.strip_prefix("data:") else {
                            continue;
                        };
                        let payload = payload.strip_prefix(' ').unwrap_or(payload);
                        if payload == "[DONE]" {
                            // 错误已记录时不再翻回成功（同包内 error 与 DONE 连发）
                            if result.error.is_none() {
                                result.success = true;
                                result.total_ms = Some(start.elapsed().as_millis() as u64);
                            }
                            terminal = true;
                            break;
                        }
                        let Ok(ev) = serde_json::from_str::<Value>(payload) else {
                            continue;
                        };
                        // 服务端随流内联错误（HTTP 200 但 data 带 error）
                        if let Some(err) = ev.get("error") {
                            let msg = err["message"].as_str().unwrap_or("未知错误");
                            finish_err(&mut result, &start, msg);
                            terminal = true;
                            break;
                        }
                        let choice = &ev["choices"][0];
                        if !choice.is_null() && !choice["finish_reason"].is_null() {
                            if result.error.is_none() {
                                result.success = true;
                                result.total_ms = Some(start.elapsed().as_millis() as u64);
                            }
                            terminal = true;
                            break;
                        }
                        let text = choice["delta"]["content"].as_str().unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let elapsed = start.elapsed().as_millis() as u64;
                        if result.first_token_ms.is_none() {
                            result.first_token_ms = Some(elapsed);
                        }
                        let used = result.content_preview.chars().count();
                        if used < 160 {
                            let remain = 160 - used;
                            result
                                .content_preview
                                .push_str(&text.chars().take(remain).collect::<String>());
                        }
                    }
                }
            }
        }
    }
    result
}

// ---------- API Key 当天用量 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    #[serde(default)]
    pub total_requests: i64,
    #[serde(default)]
    pub total_input_tokens: i64,
    #[serde(default)]
    pub total_output_tokens: i64,
    #[serde(default)]
    pub total_cache_tokens: i64,
    #[serde(default)]
    pub total_tokens: i64,
    #[serde(default)]
    pub total_cost: f64,
}

/// 指定 key 当天的用量统计（period=today 按服务器时区取自然日）
pub async fn key_usage_today(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    key_id: i64,
) -> AppResult<UsageStats> {
    let resp = http
        .get(format!(
            "{base}/api/v1/admin/usage/stats?api_key_id={key_id}&period=today"
        ))
        .bearer_auth(token)
        .send()
        .await?;
    envelope(resp).await
}

// ---------- SSE 测速 ----------

/// 对指定账号发起一次真实对话测试（SSE），客户端侧计时：
/// 请求发出 -> 首个 content 事件 = 首 token；-> test_complete/error = 总耗时。
/// 服务端不提供耗时字段，全部由本函数测量。
pub async fn test_account<F>(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    account_id: i64,
    account_name: &str,
    model: Option<&str>,
    prompt: &str,
    timeout: Duration,
    mut on_event: F,
) -> TestResult
where
    F: FnMut(TestProgress),
{
    let start = Instant::now();
    let mut result = TestResult {
        account_id,
        account_name: account_name.to_string(),
        success: false,
        model: model.unwrap_or("").to_string(),
        first_token_ms: None,
        total_ms: None,
        content_preview: String::new(),
        error: None,
    };

    let mut body = serde_json::Map::new();
    body.insert("prompt".into(), json!(prompt));
    if let Some(m) = model {
        body.insert("model_id".into(), json!(m));
    }

    let send = async {
        http.post(format!("{base}/api/v1/admin/accounts/{account_id}/test"))
            .bearer_auth(token)
            .json(&Value::Object(body))
            .send()
            .await
    };

    let resp = match tokio::time::timeout(Duration::from_secs(20), send).await {
        Err(_) => {
            finish_err(&mut result, &start, "连接超时（20 秒无响应）");
            on_event(done_msg(&result));
            return result;
        }
        Ok(Err(e)) => {
            finish_err(&mut result, &start, &format!("网络错误: {e}"));
            on_event(done_msg(&result));
            return result;
        }
        Ok(Ok(r)) => r,
    };

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let msg = match resp.json::<Value>().await {
            Ok(v) => v["message"].as_str().unwrap_or("未知错误").to_string(),
            Err(_) => format!("HTTP {status}"),
        };
        finish_err(&mut result, &start, &msg);
        on_event(done_msg(&result));
        return result;
    }

    on_event(TestProgress {
        account_id,
        kind: "start",
        model: nonempty(result.model.clone()),
        text: None,
        elapsed_ms: Some(0),
        result: None,
    });

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let deadline = start + timeout;
    let mut terminal = false;

    while !terminal {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            finish_err(&mut result, &start, &format!("测试超时（{} 秒）", timeout.as_secs()));
            break;
        }
        match tokio::time::timeout(remain, stream.next()).await {
            Err(_) => {
                if !result.success {
                    finish_err(&mut result, &start, &format!("测试超时（{} 秒）", timeout.as_secs()));
                }
                break;
            }
            Ok(None) => {
                if !result.success && result.error.is_none() {
                    finish_err(&mut result, &start, "连接中断（流提前结束）");
                }
                break;
            }
            Ok(Some(Err(e))) => {
                if !result.success && result.error.is_none() {
                    finish_err(&mut result, &start, &format!("网络错误: {e}"));
                }
                break;
            }
            Ok(Some(Ok(chunk))) => {
                buf.extend_from_slice(&chunk);
                // 提取所有完整帧（以空行 "\n\n" 结尾）
                while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                    let frame: Vec<u8> = buf.drain(..pos + 2).collect();
                    let frame_text = String::from_utf8_lossy(&frame);
                    for line in frame_text.lines() {
                        let Some(payload) = line.strip_prefix("data:") else {
                            continue;
                        };
                        let payload = payload.strip_prefix(' ').unwrap_or(payload);
                        let Ok(ev) = serde_json::from_str::<Value>(payload) else {
                            continue;
                        };
                        match ev["type"].as_str().unwrap_or("") {
                            "test_start" => {
                                if let Some(m) = ev["model"].as_str() {
                                    if !m.is_empty() {
                                        result.model = m.to_string();
                                    }
                                }
                            }
                            "content" => {
                                let elapsed = start.elapsed().as_millis() as u64;
                                if result.first_token_ms.is_none() {
                                    result.first_token_ms = Some(elapsed);
                                }
                                let text = ev["text"].as_str().unwrap_or("");
                                let used = result.content_preview.chars().count();
                                if used < 160 {
                                    let remain = 160 - used;
                                    result
                                        .content_preview
                                        .push_str(&text.chars().take(remain).collect::<String>());
                                }
                                on_event(TestProgress {
                                    account_id,
                                    kind: "content",
                                    model: None,
                                    text: Some(text.to_string()),
                                    elapsed_ms: Some(elapsed),
                                    result: None,
                                });
                            }
                            "test_complete" => {
                                // 错误已记录时不再翻回成功（同包内 error 与 complete 连发）
                                if result.error.is_none() {
                                    result.success = ev["success"].as_bool().unwrap_or(true);
                                    result.total_ms = Some(start.elapsed().as_millis() as u64);
                                }
                                terminal = true;
                            }
                            "error" => {
                                finish_err(
                                    &mut result,
                                    &start,
                                    ev["error"].as_str().unwrap_or("未知错误"),
                                );
                                terminal = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    on_event(done_msg(&result));
    result
}

fn finish_err(result: &mut TestResult, start: &Instant, msg: &str) {
    if result.error.is_none() {
        result.error = Some(msg.to_string());
    }
    result.success = false;
    result.total_ms = Some(start.elapsed().as_millis() as u64);
}

fn done_msg(result: &TestResult) -> TestProgress {
    TestProgress {
        account_id: result.account_id,
        kind: "result",
        model: None,
        text: None,
        elapsed_ms: result.total_ms,
        result: Some(result.clone()),
    }
}

fn nonempty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}
