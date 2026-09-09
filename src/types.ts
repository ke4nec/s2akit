export interface AppConfig {
  base_url: string;
  email: string;
  password: string;
  group_id: number | null;
  test_prompt: string;
  test_timeout_secs: number;
  test_concurrency: number;
  menu_opacity: number;
  /** 托盘菜单顶部用量刷新间隔（分钟），默认 10，范围 1-1440 */
  usage_refresh_minutes: number;
  /** Rust 侧维护（账号上次测试模型，account_id -> model_id），前端只读 */
  last_models?: Record<string, string>;
}

export interface AuthInfo {
  email: string;
  username: string;
}

export interface UserInfo {
  id: number;
  email: string;
  username: string;
}

export interface GroupBrief {
  id: number;
  name: string;
  platform: string;
  status: string;
  account_count: number | null;
  active_account_count: number | null;
}

export interface AccountBrief {
  id: number;
  name: string;
  platform: string;
  type: string;
  status: string;
  schedulable: boolean;
  error_message: string;
  priority: number;
  rate_limited: boolean;
  temp_unschedulable: boolean;
  group_ids: number[];
}

export interface ModelBrief {
  id: string;
  display_name: string | null;
}

export interface ComplianceInfo {
  version: string;
  phrase: string;
  document_url_zh: string;
  document_url_en: string;
}

export interface TestResult {
  account_id: number;
  account_name: string;
  success: boolean;
  model: string;
  first_token_ms: number | null;
  total_ms: number | null;
  content_preview: string;
  error: string | null;
}

export interface TestProgress {
  account_id: number;
  kind: "start" | "content" | "result";
  model: string | null;
  text: string | null;
  elapsed_ms: number | null;
  result: TestResult | null;
}

export interface LoginReply {
  user: UserInfo | null;
  requires_2fa: boolean;
  temp_token: string;
}

/** 当前使用的 API Key 当天用量（托盘菜单顶部展示） */
export interface KeyUsageToday {
  key_id: number;
  key_name: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cache_tokens: number;
  total_tokens: number;
  cost: number;
}

/// Rust AppError 序列化形态
export interface AppErrorPayload {
  message: string;
  code: string | null;
}
