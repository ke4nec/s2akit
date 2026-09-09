use crate::error::{AppError, AppResult};
use crate::state::{AppState, AuthEntry};
use crate::sub2api as api;
use crate::sub2api::{
    AccountBrief, AuthState, ComplianceInfo, GroupBrief, LoginOutcome, ModelBrief, TestProgress,
    TestResult, UserInfo,
};
use crate::config::AppConfig;
use std::future::Future;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

const DEFAULT_COMPLIANCE_PHRASE: &str =
    "I have read, understood, and agree to the Sub2API Deployment and Operation Compliance Commitment";

// ---------- 会话管理 ----------

fn store_auth(state: &AppState, auth: AuthState) {
    let expires_in = auth.expires_in.unwrap_or(86400);
    let entry = AuthEntry {
        auth,
        expires_at: Instant::now() + Duration::from_secs(expires_in),
    };
    *state.auth.write().unwrap() = Some(entry);
}

/// 刷新或用存储的凭据重新登录（不做有效期检查，始终获取新 token）
async fn refresh_or_login(app: &AppHandle) -> AppResult<String> {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    let base = cfg.base();

    let refresh_token = state
        .auth
        .read()
        .unwrap()
        .as_ref()
        .and_then(|e| e.auth.refresh_token.clone());
    if let Some(rt) = refresh_token {
        if let Ok(auth) = api::refresh(&state.http, &base, &rt).await {
            let token = auth.access_token.clone();
            store_auth(&state, auth);
            return Ok(token);
        }
    }

    if cfg.email.is_empty() || cfg.password.is_empty() {
        return Err(AppError::NotLoggedIn);
    }
    match api::login(&state.http, &base, &cfg.email, &cfg.password).await? {
        LoginOutcome::Ok(auth) => {
            let token = auth.access_token.clone();
            store_auth(&state, auth);
            Ok(token)
        }
        LoginOutcome::TwoFactorRequired { .. } => Err(AppError::other(
            "该账号开启了二步验证，请在设置页使用 2FA 验证码登录",
        )),
    }
}

async fn ensure_token(app: &AppHandle) -> AppResult<String> {
    let state = app.state::<AppState>();
    if let Some(t) = state.valid_token() {
        return Ok(t);
    }
    refresh_or_login(app).await
}

/// 带认证的 API 调用：token 过期（401）时自动刷新/重登录并重试一次。
/// 闭包捕获 http/base 等，token 由本函数注入（future 拥有 token）。
async fn authed<T, F, Fut>(app: &AppHandle, f: F) -> AppResult<T>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = AppResult<T>>,
{
    let token = ensure_token(app).await?;
    let mut out = f(token.clone()).await;
    if out.is_err() && out.as_ref().err().map(|e| e.is_unauthorized()).unwrap_or(false) {
        let token = refresh_or_login(app).await?;
        out = f(token).await;
    }
    out
}

// ---------- 公共任务 ----------

/// 重新加载当前目标分组的账号并刷新托盘菜单，向前端广播
pub async fn refresh_accounts_task(app: &AppHandle) -> AppResult<Vec<AccountBrief>> {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    let gid = cfg.group_id;
    let http = state.http.clone();
    let base = cfg.base();

    let accounts = authed(app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::list_accounts(&http, &base, &token, gid).await }
    })
    .await?;

    let group_name = {
        let state = app.state::<AppState>();
        let groups = state.groups.read().unwrap();
        match gid {
            Some(id) => groups
                .iter()
                .find(|g| g.id == id)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| format!("#{id}")),
            None => "全部账号".to_string(),
        }
    };

    let state = app.state::<AppState>();
    *state.accounts.write().unwrap() = accounts.clone();
    *state.group_name.write().unwrap() = group_name;
    drop(state);
    let _ = app.emit("accounts-updated", &accounts);
    Ok(accounts)
}

/// 托盘菜单「切换账号」：启用目标账号并通知
#[allow(dead_code)]
pub async fn switch_account_task(app: &AppHandle, account_id: i64) {
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let base = state.config_snapshot().base();
    let name = state
        .accounts
        .read()
        .unwrap()
        .iter()
        .find(|a| a.id == account_id)
        .map(|a| a.name.clone())
        .unwrap_or_else(|| format!("#{account_id}"));
    drop(state);

    let res = authed(app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::set_schedulable(&http, &base, &token, account_id, true).await }
    })
    .await;

    match res {
        Ok(()) => {
            notify(app, &format!("已启用账号：{name}"));
            let _ = refresh_accounts_task(app).await;
        }
        Err(e) => {
            notify(app, &format!("启用 {name} 失败：{e}"));
        }
    }
}

/// 托盘菜单「测试该账号」：单账号测试并系统通知结果，返回结果供菜单行内展示
pub async fn test_account_task(
    app: AppHandle,
    account_id: i64,
    model: Option<String>,
) -> TestResult {
    let account = {
        let state = app.state::<AppState>();
        let found = state
            .accounts
            .read()
            .unwrap()
            .iter()
            .find(|a| a.id == account_id)
            .cloned();
        drop(state);
        match found {
            Some(a) => a,
            None => {
                let msg = format!("账号 #{account_id} 不在当前分组中");
                notify(&app, &msg);
                return TestResult {
                    account_id,
                    account_name: format!("#{account_id}"),
                    success: false,
                    model: String::new(),
                    first_token_ms: None,
                    total_ms: None,
                    content_preview: String::new(),
                    error: Some(msg),
                };
            }
        }
    };
    let model = match model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()) {
        Some(m) => Some(m),
        None => resolve_model(&app, account_id, &account.platform)
            .await
            .unwrap_or(None),
    };
    let result = test_one(&app, &account, model, None).await;
    if result.success {
        notify(
            &app,
            &format!(
                "{} 测试成功：首 token {} ms（{}）",
                result.account_name,
                result.first_token_ms.map(|m| m.to_string()).unwrap_or_else(|| "—".into()),
                result.model,
            ),
        );
    } else {
        notify(
            &app,
            &format!(
                "{} 测试失败：{}",
                result.account_name,
                result.error.as_deref().unwrap_or("未知错误"),
            ),
        );
    }
    result
}

/// 托盘右键「测试该账号」入口：返回结果供前端行内展示，同时发系统通知
#[tauri::command]
pub async fn tray_test_account(
    app: AppHandle,
    account_id: i64,
    model: Option<String>,
) -> TestResult {
    test_account_task(app, account_id, model).await
}

/// 最近一次测试结果（account_id -> 结果），供托盘菜单行内展示
#[tauri::command]
pub fn get_last_results(
    state: State<'_, AppState>,
) -> std::collections::HashMap<i64, TestResult> {
    state.last_results.read().unwrap().clone()
}

fn notify(app: &AppHandle, body: &str) {
    // Windows 直发并指定 AUMID,避免插件在开发/便携模式下回退 PowerShell 图标
    #[cfg(windows)]
    {
        crate::win_toast::notify(app, "s2akit", body);
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = app
            .notification()
            .builder()
            .title("s2akit")
            .body(body)
            .show();
    }
}

// ---------- 测速 ----------

fn is_media_model(id: &str) -> bool {
    let id = id.to_lowercase();
    ["image", "video", "audio", "tts", "whisper", "sora", "realtime", "dall", "veo"]
        .iter()
        .any(|k| id.contains(k))
}

/// 解析账号的测试模型（未显式指定时），候选均须存在于账号模型列表中：
/// 1) 该账号上次测试实际使用的模型（持久化在配置里）
/// 2) 平台默认：openai→astra；anthropic/antigravity→opus-5 优先、opus 次之
/// 3) 列表中第一个非媒体模型
async fn resolve_model(
    app: &AppHandle,
    account_id: i64,
    platform: &str,
) -> AppResult<Option<String>> {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    let last = cfg.last_models.get(&account_id).cloned();
    let http = state.http.clone();
    let base = cfg.base();
    drop(state);

    let models = authed(app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::account_models(&http, &base, &token, account_id).await }
    })
    .await
    .unwrap_or_default();
    let ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
    let contains = |m: &str| ids.iter().any(|x| x == m);

    if let Some(last) = last {
        if contains(&last) {
            return Ok(Some(last));
        }
    }

    let keywords: &[&str] = match platform {
        "openai" => &["astra"],
        "anthropic" | "antigravity" => &["opus-5", "opus"],
        _ => &[],
    };
    let lower: Vec<String> = ids.iter().map(|s| s.to_lowercase()).collect();
    for k in keywords {
        if let Some(idx) = lower
            .iter()
            .position(|id| id.contains(k) && !is_media_model(id))
        {
            return Ok(Some(ids[idx].clone()));
        }
    }
    Ok(ids.iter().find(|m| !is_media_model(m)).cloned())
}

/// 记录账号本次测试使用的模型，作为下次默认（锁内读改写，避免批量测试并发时互相覆盖）
fn record_last_model(app: &AppHandle, account_id: i64, model: &str) {
    let state = app.state::<AppState>();
    let mut cfg = state.config.write().unwrap();
    cfg.last_models.insert(account_id, model.to_string());
    let snapshot = cfg.clone();
    let path = state.config_path.clone();
    drop(cfg);
    let _ = snapshot.save(&path);
}

#[allow(clippy::too_many_arguments)]
async fn test_one(
    app: &AppHandle,
    account: &AccountBrief,
    model: Option<String>,
    channel: Option<&Channel<TestProgress>>,
) -> TestResult {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    let http = state.http.clone();
    let base = cfg.base();
    let timeout = Duration::from_secs(cfg.test_timeout_secs.max(5));
    let prompt = if cfg.test_prompt.trim().is_empty() {
        "hi".to_string()
    } else {
        cfg.test_prompt.trim().to_string()
    };
    drop(state);

    let token = match ensure_token(app).await {
        Ok(t) => t,
        Err(e) => {
            return TestResult {
                account_id: account.id,
                account_name: account.name.clone(),
                success: false,
                model: String::new(),
                first_token_ms: None,
                total_ms: None,
                content_preview: String::new(),
                error: Some(e.to_string()),
            }
        }
    };

    let app_h = app.clone();
    let channel = channel.cloned();
    let on_event = move |msg: TestProgress| {
        let _ = app_h.emit("test-progress", msg.clone());
        if let Some(ch) = &channel {
            let _ = ch.send(msg);
        }
    };

    let result = api::test_account(
        &http,
        &base,
        &token,
        account.id,
        &account.name,
        model.as_deref(),
        &prompt,
        timeout,
        on_event,
    )
    .await;
    // 记住本次实际使用的模型，作为该账号下次测试的默认
    if !result.model.is_empty() {
        record_last_model(app, account.id, &result.model);
    }
    // 缓存最近结果（托盘悬停子菜单展示）并刷新托盘菜单
    app.state::<AppState>()
        .last_results
        .write()
        .unwrap()
        .insert(account.id, result.clone());
    result
}

// ---------- 启动初始化 ----------

pub async fn startup_init(app: AppHandle) {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    if cfg.email.is_empty() || cfg.password.is_empty() {
        return;
    }
    let base = cfg.base();
    match api::login(&state.http, &base, &cfg.email, &cfg.password).await {
        Ok(LoginOutcome::Ok(auth)) => {
            store_auth(&state, auth);
            drop(state);
            let _ = app.emit("auth-changed", ());
        }
        other => {
            eprintln!(
                "startup auto-login skipped: {}",
                other.err().map(|e| e.to_string()).unwrap_or_else(|| "2FA required".into())
            );
            return;
        }
    }

    let http = app.state::<AppState>().http.clone();
    let base = app.state::<AppState>().base();
    if let Ok(groups) = authed(&app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::list_groups(&http, &base, &token).await }
    })
    .await
    {
        *app.state::<AppState>().groups.write().unwrap() = groups;
    }
    let _ = refresh_accounts_task(&app).await;
}

// ---------- 托盘菜单窗口透明度 ----------

/// tauri 核心未提供窗口不透明度 API，Windows 下用分层窗口 (WS_EX_LAYERED + LWA_ALPHA) 实现
#[cfg(windows)]
pub(crate) fn set_window_alpha_win32(w: &tauri::WebviewWindow, opacity: f32) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE,
        LWA_ALPHA, WS_EX_LAYERED,
    };
    if let Ok(hwnd) = w.hwnd() {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
            let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha as u8, LWA_ALPHA);
        }
    }
}

/// 按已保存配置给托盘菜单窗口设置不透明度（每次显示前调用，幂等）
pub fn apply_menu_opacity(app: &AppHandle) {
    let opacity = app
        .state::<AppState>()
        .config_snapshot()
        .menu_opacity
        .clamp(0.3, 1.0);
    if let Some(w) = app.get_webview_window("tray-menu") {
        #[cfg(windows)]
        set_window_alpha_win32(&w, opacity);
        #[cfg(not(windows))]
        let _ = &w;
    }
}

#[tauri::command]
pub fn set_menu_opacity(app: AppHandle, opacity: f32) -> AppResult<()> {
    let opacity = opacity.clamp(0.3, 1.0);
    let state = app.state::<AppState>();
    let mut cfg = state.config_snapshot();
    cfg.menu_opacity = opacity;
    state.save_config(&cfg)?;
    drop(state);
    apply_menu_opacity(&app);
    Ok(())
}

#[tauri::command]
pub fn open_main_window(app: AppHandle) {
    crate::tray::show_main_window(&app);
}

/// 托盘菜单前端的心跳应答：页面成功执行 JS 即视为存活
#[tauri::command]
pub fn menu_pong(app: AppHandle) {
    app.state::<AppState>()
        .menu_alive
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 托盘菜单前端回报真实内容高度：窗口收缩贴合内容，并按锚点重定位
/// （菜单底角始终贴住右键点击位置，与原生托盘菜单一致）
#[tauri::command]
pub fn fit_menu(app: AppHandle, height: f64) {
    crate::tray::fit_menu_window(&app, height);
}

/// 显示账号级联子菜单独立窗口：主菜单窗口宽度不变，子菜单另起窗口级联在主菜单旁
#[tauri::command]
pub fn show_submenu(app: AppHandle, account_id: i64, row_top: f64, height: f64) {
    crate::tray::show_submenu_window(&app, account_id, row_top, height);
}

/// 显示分组级联子菜单独立窗口（与账号子菜单共用窗口，主菜单宽度不变）
#[tauri::command]
pub fn show_groups_submenu(app: AppHandle, row_top: f64, height: f64) {
    crate::tray::show_groups_submenu_window(&app, row_top, height);
}

/// 隐藏账号级联子菜单窗口
#[tauri::command]
pub fn hide_submenu(app: AppHandle) {
    crate::tray::hide_submenu_window(&app);
}

/// 级联子菜单前端回报真实内容高度：重设子菜单窗口尺寸并按行锚点重定位
#[tauri::command]
pub fn fit_submenu(app: AppHandle, height: f64) {
    crate::tray::fit_submenu_window(&app, height);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---------- Commands ----------

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppConfig {
    state.config_snapshot()
}

#[tauri::command]
pub async fn save_config(app: AppHandle, mut config: AppConfig) -> AppResult<()> {
    let state = app.state::<AppState>();
    let old = state.config_snapshot();
    // last_models 由测试流程实时更新、menu_opacity 由设置页单独命令控制，
    // 均以本地当前值为准，防止前端旧快照保存设置时把它们覆盖回旧值
    config.last_models = old.last_models.clone();
    config.menu_opacity = old.menu_opacity;
    config.usage_refresh_minutes = config
        .usage_refresh_minutes
        .clamp(AppConfig::MIN_USAGE_REFRESH_MINUTES, AppConfig::MAX_USAGE_REFRESH_MINUTES);
    state.save_config(&config)?;
    drop(state);
    if old.base_url != config.base_url
        || old.email != config.email
        || old.password != config.password
    {
        *app.state::<AppState>().auth.write().unwrap() = None;
        let _ = app.emit("auth-changed", ());
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LoginReply {
    pub user: Option<UserInfo>,
    pub requires_2fa: bool,
    pub temp_token: String,
}

#[tauri::command]
pub async fn login(
    app: AppHandle,
    email: Option<String>,
    password: Option<String>,
) -> AppResult<LoginReply> {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    let email = email
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.email.clone());
    let password = password
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.password.clone());
    if email.is_empty() || password.is_empty() {
        return Err(AppError::other("请先填写邮箱和密码"));
    }
    let base = cfg.base();
    match api::login(&state.http, &base, &email, &password).await? {
        LoginOutcome::Ok(auth) => {
            let user = auth.user.clone();
            store_auth(&state, auth);
            drop(state);
            let _ = app.emit("auth-changed", ());
            Ok(LoginReply {
                user: Some(user),
                requires_2fa: false,
                temp_token: String::new(),
            })
        }
        LoginOutcome::TwoFactorRequired { temp_token } => Ok(LoginReply {
            user: None,
            requires_2fa: true,
            temp_token,
        }),
    }
}

#[tauri::command]
pub async fn login_2fa(app: AppHandle, temp_token: String, code: String) -> AppResult<UserInfo> {
    let state = app.state::<AppState>();
    let base = state.config_snapshot().base();
    let auth = api::login_2fa(&state.http, &base, &temp_token, &code.trim()).await?;
    let user = auth.user.clone();
    store_auth(&state, auth);
    drop(state);
    let _ = app.emit("auth-changed", ());
    Ok(user)
}

#[tauri::command]
pub async fn logout(app: AppHandle) {
    *app.state::<AppState>().auth.write().unwrap() = None;
    let _ = app.emit("auth-changed", ());
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthInfo {
    pub email: String,
    pub username: String,
}

#[tauri::command]
pub fn get_auth(state: State<'_, AppState>) -> Option<AuthInfo> {
    state.auth.read().unwrap().as_ref().map(|e| AuthInfo {
        email: e.auth.user.email.clone(),
        username: e.auth.user.username.clone(),
    })
}

#[tauri::command]
pub async fn get_compliance(app: AppHandle) -> AppResult<ComplianceInfo> {
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let base = state.config_snapshot().base();
    drop(state);
    authed(&app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::get_compliance(&http, &base, &token).await }
    })
    .await
}

#[tauri::command]
pub async fn accept_compliance(app: AppHandle) -> AppResult<()> {
    let info = get_compliance(app.clone()).await?;
    let phrase = if info.phrase.is_empty() {
        DEFAULT_COMPLIANCE_PHRASE.to_string()
    } else {
        info.phrase
    };
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let base = state.config_snapshot().base();
    drop(state);
    authed(&app, move |token| {
        let http = http.clone();
        let base = base.clone();
        let phrase = phrase.clone();
        async move { api::accept_compliance(&http, &base, &token, &phrase).await }
    })
    .await
}

#[tauri::command]
pub async fn list_groups(app: AppHandle) -> AppResult<Vec<GroupBrief>> {
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let base = state.config_snapshot().base();
    drop(state);
    let groups = authed(&app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::list_groups(&http, &base, &token).await }
    })
    .await?;
    *app.state::<AppState>().groups.write().unwrap() = groups.clone();
    Ok(groups)
}

#[tauri::command]
pub async fn list_accounts(app: AppHandle, group_id: Option<i64>) -> AppResult<Vec<AccountBrief>> {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    let gid = group_id.or(cfg.group_id);
    let http = state.http.clone();
    let base = cfg.base();
    drop(state);

    let accounts = authed(&app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::list_accounts(&http, &base, &token, gid).await }
    })
    .await?;

    // 当前目标分组的列表才更新托盘缓存
    let is_selected = match group_id {
        Some(g) => app.state::<AppState>().config_snapshot().group_id == Some(g),
        None => true,
    };
    if is_selected {
        *app.state::<AppState>().accounts.write().unwrap() = accounts.clone();
        let _ = app.emit("accounts-updated", &accounts);
    }
    Ok(accounts)
}

#[tauri::command]
pub async fn select_group(app: AppHandle, group_id: i64) -> AppResult<()> {
    let state = app.state::<AppState>();
    let mut cfg = state.config_snapshot();
    cfg.group_id = Some(group_id);
    state.save_config(&cfg)?;
    drop(state);
    refresh_accounts_task(&app).await.map(|_| ())
}

#[tauri::command]
pub async fn get_account_models(app: AppHandle, account_id: i64) -> AppResult<Vec<ModelBrief>> {
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let base = state.config_snapshot().base();
    drop(state);
    authed(&app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::account_models(&http, &base, &token, account_id).await }
    })
    .await
}

#[tauri::command]
pub async fn set_schedulable(
    app: AppHandle,
    account_id: i64,
    schedulable: bool,
) -> AppResult<Vec<AccountBrief>> {
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let base = state.config_snapshot().base();
    drop(state);

    authed(&app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::set_schedulable(&http, &base, &token, account_id, schedulable).await }
    })
    .await?;

    // 启用/禁用不打系统通知：开关状态本身即反馈，失败走 Err 由前端 snackbar 提示
    refresh_accounts_task(&app).await
}

#[tauri::command]
pub async fn test_account(
    app: AppHandle,
    account_id: i64,
    model: Option<String>,
    on_event: Channel<TestProgress>,
) -> AppResult<TestResult> {
    let state = app.state::<AppState>();
    let account = state
        .accounts
        .read()
        .unwrap()
        .iter()
        .find(|a| a.id == account_id)
        .cloned()
        .unwrap_or(AccountBrief {
            id: account_id,
            name: format!("#{account_id}"),
            platform: String::new(),
            account_type: String::new(),
            status: String::new(),
            schedulable: false,
            error_message: String::new(),
            priority: 0,
            rate_limited: false,
            temp_unschedulable: false,
            group_ids: vec![],
        });
    drop(state);

    let model = match model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()) {
        Some(m) => Some(m),
        None => resolve_model(&app, account_id, &account.platform).await?,
    };
    Ok(test_one(&app, &account, model, Some(&on_event)).await)
}

// ---------- 当前 Key 当天用量 ----------

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

/// 托盘菜单顶部展示：最近使用的 API Key 当天的用量（费用 + token 量）。
/// 结果按配置的刷新间隔（usage_refresh_minutes，默认 10 分钟）缓存，
/// 间隔内重复打开菜单直接返回缓存；没有任何用量记录时返回 None（前端隐藏该行）。
/// force=true 绕过缓存强制刷新（托盘菜单"刷新账号列表"用）。
#[tauri::command]
pub async fn get_key_usage_today(
    app: AppHandle,
    force: Option<bool>,
) -> AppResult<Option<KeyUsageToday>> {
    let state = app.state::<AppState>();
    let ttl = Duration::from_secs(state.config_snapshot().usage_refresh_minutes_clamped() * 60);
    if !force.unwrap_or(false) {
        if let Some(cache) = state.usage_cache.read().unwrap().as_ref() {
            if cache.fetched_at.elapsed() < ttl {
                return Ok(cache.usage.clone());
            }
        }
    }
    let (http, base) = (state.http.clone(), state.config_snapshot().base());
    drop(state);

    let usage = match fetch_usage(&app, http, base).await {
        Ok(usage) => usage,
        // 拉取失败时退回旧缓存（宁可显示过期数据也不闪空），无缓存才报错
        Err(e) => {
            return match app.state::<AppState>().usage_cache.read().unwrap().clone() {
                Some(cache) => Ok(cache.usage),
                None => Err(e),
            }
        }
    };
    cache_usage(&app, usage.clone());
    Ok(usage)
}

async fn fetch_usage(
    app: &AppHandle,
    http: reqwest::Client,
    base: String,
) -> AppResult<Option<KeyUsageToday>> {
    let http2 = http.clone();
    let base2 = base.clone();
    let key = authed(app, move |token| {
        let http = http2.clone();
        let base = base2.clone();
        async move { api::current_api_key(&http, &base, &token).await }
    })
    .await?;
    let Some(key) = key else {
        return Ok(None);
    };

    let stats = authed(app, move |token| {
        let http = http.clone();
        let base = base.clone();
        async move { api::key_usage_today(&http, &base, &token, key.id).await }
    })
    .await?;

    Ok(Some(KeyUsageToday {
        key_id: key.id,
        key_name: key.name,
        requests: stats.total_requests,
        input_tokens: stats.total_input_tokens,
        output_tokens: stats.total_output_tokens,
        cache_tokens: stats.total_cache_tokens,
        total_tokens: stats.total_tokens,
        cost: stats.total_cost,
    }))
}

/// 记录/覆盖用量缓存（无任何用量记录的 None 也缓存，同样受间隔约束）
fn cache_usage(app: &AppHandle, usage: Option<KeyUsageToday>) {
    *app.state::<AppState>().usage_cache.write().unwrap() = Some(crate::state::UsageCache {
        fetched_at: Instant::now(),
        usage,
    });
}
