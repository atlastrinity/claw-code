use crate::error::ApiError;
use crate::key_rotation;
use crate::prompt_cache::{PromptCache, PromptCacheRecord, PromptCacheStats};
use crate::providers::anthropic::{self, AnthropicClient, AuthSource};
use crate::providers::openai_compat::{self, OpenAiCompatClient, OpenAiCompatConfig};
use crate::providers::{self, ProviderKind};
use crate::types::{MessageRequest, MessageResponse, StreamEvent};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

fn get_active_key_index(model: &str) -> usize {
    static ACTIVE_KEYS: OnceLock<RwLock<HashMap<String, usize>>> = OnceLock::new();
    let map = ACTIVE_KEYS.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(guard) = map.read() {
        if let Some(&idx) = guard.get(model) {
            return idx;
        }
    }
    1
}

fn set_active_key_index(model: &str, index: usize) {
    static ACTIVE_KEYS: OnceLock<RwLock<HashMap<String, usize>>> = OnceLock::new();
    let map = ACTIVE_KEYS.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut guard) = map.write() {
        guard.insert(model.to_string(), index);
    }
}

struct TpmRateLimiter {
    window: Mutex<Vec<(Instant, usize)>>,
}

impl TpmRateLimiter {
    fn new() -> Self {
        Self {
            window: Mutex::new(Vec::new()),
        }
    }

    /// Reserve `estimated_tokens` against `limit`, waiting only as long as the
    /// budget actually requires. Returns the time spent waiting.
    ///
    /// The wait is the time until *just enough* recorded usage leaves the 60 s
    /// window, not until the oldest entry does: waiting for the oldest entry
    /// overshoots by the whole span of the entries in between (measured: 14–43 s
    /// stalls between requests that needed far less).
    async fn acquire(&self, limit: usize, estimated_tokens: usize) -> Duration {
        // A single request larger than the whole budget can never fit. Waiting
        // would only drain the window and then admit it anyway, so admit it now
        // and say so — the provider's own 429 is the authority here, not us.
        if estimated_tokens > limit {
            tracing::warn!(
                estimated_tokens,
                limit,
                "request exceeds the entire TPM budget; admitting without waiting"
            );
            self.record(estimated_tokens);
            return Duration::ZERO;
        }

        let mut waited = Duration::ZERO;
        loop {
            let wait_duration = {
                let now = Instant::now();
                let mut lock = match self.window.lock() {
                    Ok(g) => g,
                    Err(_) => return waited,
                };

                lock.retain(|(time, _)| now.duration_since(*time) < Duration::from_secs(60));

                let current_tokens: usize = lock.iter().map(|(_, tokens)| tokens).sum();
                if current_tokens + estimated_tokens <= limit {
                    lock.push((now, estimated_tokens));
                    return waited;
                }

                // Oldest first: each entry leaves the window at `time + 60s`, so
                // the first prefix whose removal frees enough budget defines the
                // earliest instant this request can go out.
                let entries: Vec<(Instant, usize)> = lock.iter().copied().collect();
                match minimal_wait(&entries, current_tokens, limit, estimated_tokens, now) {
                    Some(wait) => wait,
                    // Nothing in the window can free enough (only possible when
                    // the budget is smaller than one request, handled above).
                    None => {
                        lock.push((now, estimated_tokens));
                        return waited;
                    }
                }
            };

            tokio::time::sleep(wait_duration).await;
            waited += wait_duration;
        }
    }

    fn record(&self, estimated_tokens: usize) {
        if let Ok(mut lock) = self.window.lock() {
            lock.push((Instant::now(), estimated_tokens));
        }
    }
}

/// How long to wait before `estimated_tokens` fits in `limit`, given the usage
/// already recorded in the window. `None` means no amount of waiting helps.
///
/// Waiting for the *oldest* entry to expire — the obvious implementation —
/// overshoots by the span of everything recorded after it, which is what turned
/// a 20 000-token budget and a 28 943-token request into 14–43 s stalls.
fn minimal_wait(
    entries: &[(Instant, usize)],
    current_tokens: usize,
    limit: usize,
    estimated_tokens: usize,
    now: Instant,
) -> Option<Duration> {
    if current_tokens + estimated_tokens <= limit {
        return Some(Duration::ZERO);
    }
    let mut ordered: Vec<(Instant, usize)> = entries.to_vec();
    ordered.sort_unstable_by_key(|(time, _)| *time);
    let mut freed = 0usize;
    for (time, tokens) in ordered {
        freed += tokens;
        if current_tokens - freed + estimated_tokens <= limit {
            return Some((time + Duration::from_secs(60)).saturating_duration_since(now));
        }
    }
    None
}

static GLM_LIMITER: OnceLock<TpmRateLimiter> = OnceLock::new();
static GEMINI_LIMITER: OnceLock<TpmRateLimiter> = OnceLock::new();
static DEFAULT_LIMITER: OnceLock<TpmRateLimiter> = OnceLock::new();

/// When the previous request was scheduled to start, for the
/// `CLAW_API_PAUSE_SECS` floor.
static LAST_REQUEST_START: Mutex<Option<Instant>> = Mutex::new(None);

/// Sleep only the remainder of the configured minimum interval and return how
/// long that was.
///
/// `CLAW_API_PAUSE_SECS` is a floor on the spacing between request *starts*, not
/// a tax on every request. Sleeping the full amount unconditionally adds pure
/// dead time whenever the previous request plus the tool work after it already
/// took longer than the floor — measured as exactly 5.000 s gaps between
/// requests in a turn that had spent 30 s executing tools (362 s of dead time in
/// one turn).
async fn apply_minimum_interval() -> Duration {
    let Some(interval) = configured_minimum_interval() else {
        return Duration::ZERO;
    };
    let now = Instant::now();
    let wait = {
        let mut last = match LAST_REQUEST_START.lock() {
            Ok(g) => g,
            Err(_) => return Duration::ZERO,
        };
        let wait = last
            .map(|previous| (previous + interval).saturating_duration_since(now))
            .unwrap_or(Duration::ZERO);
        *last = Some(now + wait);
        wait
    };
    if wait > Duration::ZERO {
        tokio::time::sleep(wait).await;
    }
    wait
}

/// `CLAW_API_PAUSE_SECS` as a duration, or `None` when unset, unparseable or 0.
fn configured_minimum_interval() -> Option<Duration> {
    minimum_interval_from(std::env::var("CLAW_API_PAUSE_SECS").ok().as_deref())
}

fn minimum_interval_from(raw: Option<&str>) -> Option<Duration> {
    let secs = raw?.trim().parse::<u64>().ok()?;
    (secs > 0).then(|| Duration::from_secs(secs))
}

#[cfg(test)]
fn reset_minimum_interval() {
    if let Ok(mut last) = LAST_REQUEST_START.lock() {
        *last = None;
    }
}

pub(crate) fn estimate_request_tokens(request: &MessageRequest) -> usize {
    let mut total_bytes = 0;
    if let Some(ref system) = request.system {
        total_bytes += system.len();
    }
    for msg in &request.messages {
        for block in &msg.content {
            match block {
                crate::types::InputContentBlock::Text { text } => {
                    total_bytes += text.len();
                }
                crate::types::InputContentBlock::Thinking { thinking, .. } => {
                    total_bytes += thinking.len();
                }
                crate::types::InputContentBlock::ToolUse { name, input, .. } => {
                    total_bytes += name.len() + input.to_string().len();
                }
                crate::types::InputContentBlock::ToolResult { content, .. } => {
                    for rblock in content {
                        match rblock {
                            crate::types::ToolResultContentBlock::Text { text } => {
                                total_bytes += text.len();
                            }
                            crate::types::ToolResultContentBlock::Json { value } => {
                                total_bytes += value.to_string().len();
                            }
                        }
                    }
                }
            }
        }
    }
    total_bytes / 2
}

pub(crate) struct ApiLockGuard {
    lock_path: std::path::PathBuf,
}

impl ApiLockGuard {
    fn new(home: &str) -> Self {
        let lock_path = std::path::Path::new(home).join(".claw/api.lock");
        if let Some(parent) = lock_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(meta) = std::fs::metadata(&lock_path) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().unwrap_or_default().as_secs() > 15 {
                    let _ = std::fs::remove_file(&lock_path);
                }
            }
        }
        let _ = std::fs::File::create(&lock_path);
        Self { lock_path }
    }
}

impl Drop for ApiLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ProviderClient {
    Anthropic(AnthropicClient),
    Xai(OpenAiCompatClient),
    OpenAi(OpenAiCompatClient),
}

impl ProviderClient {
    pub fn from_model(model: &str) -> Result<Self, ApiError> {
        let resolved = providers::resolve_model_alias(model);
        let active_index = get_active_key_index(&resolved);
        if let Ok(client) = Self::from_model_with_key_index(&resolved, active_index) {
            Ok(client)
        } else {
            Self::from_model_with_anthropic_auth(model, None)
        }
    }

    pub fn from_model_with_anthropic_auth(
        model: &str,
        anthropic_auth: Option<AuthSource>,
    ) -> Result<Self, ApiError> {
        let resolved_model = providers::resolve_model_alias(model);
        let client = Self::unbound_client_for_auth(&resolved_model, anthropic_auth)?;
        Ok(client.bound_to_model(&resolved_model))
    }

    fn unbound_client_for_auth(
        resolved_model: &str,
        anthropic_auth: Option<AuthSource>,
    ) -> Result<Self, ApiError> {
        if let Some(auth) = anthropic_auth {
            return Ok(Self::Anthropic(AnthropicClient::from_auth(auth)));
        }
        match providers::detect_provider_kind(resolved_model) {
            ProviderKind::Anthropic => Ok(Self::Anthropic(AnthropicClient::from_env()?)),
            ProviderKind::Xai => Ok(Self::Xai(OpenAiCompatClient::from_env(
                OpenAiCompatConfig::xai(),
            )?)),
            ProviderKind::OpenAi => {
                let metadata = providers::metadata_for_model(resolved_model);
                if metadata.is_none() && std::env::var_os("OLLAMA_HOST").is_some() {
                    Ok(Self::OpenAi(
                        openai_compat::OpenAiCompatClient::from_ollama_env()
                            .expect("from_ollama_env always returns Some"),
                    ))
                } else {
                    let config = match metadata {
                        Some(meta) if meta.auth_env == "DASHSCOPE_API_KEY" => {
                            OpenAiCompatConfig::dashscope()
                        }
                        Some(meta) if meta.auth_env == "GLM_API_KEY" => OpenAiCompatConfig::glm(),
                        Some(meta) if meta.auth_env == "CLOUDFLARE_API_TOKEN" => {
                            OpenAiCompatConfig::cloudflare()
                        }
                        Some(meta) if meta.auth_env == "NVIDIA_API_KEY" => {
                            OpenAiCompatConfig::nvidia()
                        }
                        Some(meta) if meta.auth_env == "GEMINI_API_KEY" => {
                            OpenAiCompatConfig::gemini()
                        }
                        Some(meta) if meta.auth_env == "SILICONFLOW_API_KEY" => {
                            OpenAiCompatConfig::siliconflow()
                        }
                        Some(meta) if meta.auth_env == "MISTRAL_API_KEY" => {
                            OpenAiCompatConfig::mistral()
                        }
                        _ => OpenAiCompatConfig::openai(),
                    };
                    Ok(Self::OpenAi(OpenAiCompatClient::from_env(config)?))
                }
            }
        }
    }

    pub fn has_key_for_index(model: &str, key_index: usize) -> bool {
        if key_index == 0 {
            return false;
        }
        let resolved_model = providers::resolve_model_alias(model);
        let api_key_env = match providers::detect_provider_kind(&resolved_model) {
            ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
            ProviderKind::Xai => "XAI_API_KEY",
            ProviderKind::OpenAi => match providers::metadata_for_model(&resolved_model) {
                Some(meta) => meta.auth_env,
                None => "OPENAI_API_KEY",
            },
        };
        key_rotation::has_key_at_index(api_key_env, key_index)
    }

    pub fn from_model_with_key_index(model: &str, key_index: usize) -> Result<Self, ApiError> {
        let resolved_model = providers::resolve_model_alias(model);
        let client = Self::client_for_model_at_index(&resolved_model, key_index)?;
        Ok(client.bound_to_model(&resolved_model))
    }

    /// Bind this client's δ breaker to the one shared by every client serving
    /// `model`, so rotation and reconnects accumulate escalation instead of
    /// restarting at level 0.
    fn bound_to_model(self, model: &str) -> Self {
        match self {
            Self::Anthropic(client) => Self::Anthropic(client.with_breaker_model(model)),
            Self::Xai(client) => Self::Xai(client.with_breaker_model(model)),
            Self::OpenAi(client) => Self::OpenAi(client.with_breaker_model(model)),
        }
    }

    fn client_for_model_at_index(resolved_model: &str, key_index: usize) -> Result<Self, ApiError> {
        match providers::detect_provider_kind(resolved_model) {
            ProviderKind::Anthropic => {
                let auth_env = "ANTHROPIC_API_KEY";
                let api_key = key_rotation::key_at_index(auth_env, key_index);
                let auth_token = key_rotation::key_at_index("ANTHROPIC_AUTH_TOKEN", key_index);

                let auth_source = match (api_key, auth_token) {
                    (Some(api_key), Some(bearer_token)) => AuthSource::ApiKeyAndBearer {
                        api_key,
                        bearer_token,
                    },
                    (Some(api_key), None) => AuthSource::ApiKey(api_key),
                    (None, Some(bearer_token)) => AuthSource::BearerToken(bearer_token),
                    (None, None) => {
                        return Err(ApiError::Auth("Missing ANTHROPIC_API_KEY".to_string()))
                    }
                };

                let mut client = AnthropicClient::from_auth(auth_source);

                // For base URL, try numbered env var, then default
                let base_url_keys = key_rotation::parse_keys("ANTHROPIC_BASE_URL");
                if let Some(url) = base_url_keys.into_iter().nth(key_index.saturating_sub(1)) {
                    if !url.trim().is_empty() {
                        client = client.with_base_url(url);
                    }
                }

                Ok(Self::Anthropic(client))
            }
            ProviderKind::Xai => {
                let config = OpenAiCompatConfig::xai();
                let api_key =
                    key_rotation::key_at_index(config.api_key_env, key_index).unwrap_or_default();
                if api_key.is_empty() {
                    return Err(ApiError::Auth(format!(
                        "Missing credentials for provider: {}",
                        config.provider_name
                    )));
                }
                let base_url_keys = key_rotation::parse_keys(config.base_url_env);
                let base_url = base_url_keys
                    .into_iter()
                    .nth(key_index.saturating_sub(1))
                    .unwrap_or_else(|| {
                        std::env::var(config.base_url_env)
                            .unwrap_or_else(|_| config.default_base_url.to_string())
                    });
                let client = OpenAiCompatClient::new(api_key, config).with_base_url(base_url);
                Ok(Self::Xai(client))
            }
            ProviderKind::OpenAi => {
                let metadata = providers::metadata_for_model(resolved_model);
                if metadata.is_none() && std::env::var_os("OLLAMA_HOST").is_some() {
                    Ok(Self::OpenAi(
                        openai_compat::OpenAiCompatClient::from_ollama_env()
                            .expect("from_ollama_env always returns Some"),
                    ))
                } else {
                    let config = match metadata {
                        Some(meta) if meta.auth_env == "DASHSCOPE_API_KEY" => {
                            OpenAiCompatConfig::dashscope()
                        }
                        Some(meta) if meta.auth_env == "GLM_API_KEY" => OpenAiCompatConfig::glm(),
                        Some(meta) if meta.auth_env == "CLOUDFLARE_API_TOKEN" => {
                            OpenAiCompatConfig::cloudflare()
                        }
                        Some(meta) if meta.auth_env == "NVIDIA_API_KEY" => {
                            OpenAiCompatConfig::nvidia()
                        }
                        Some(meta) if meta.auth_env == "GEMINI_API_KEY" => {
                            OpenAiCompatConfig::gemini()
                        }
                        Some(meta) if meta.auth_env == "SILICONFLOW_API_KEY" => {
                            OpenAiCompatConfig::siliconflow()
                        }
                        Some(meta) if meta.auth_env == "MISTRAL_API_KEY" => {
                            OpenAiCompatConfig::mistral()
                        }
                        _ => OpenAiCompatConfig::openai(),
                    };

                    let api_key = key_rotation::key_at_index(config.api_key_env, key_index)
                        .unwrap_or_default();

                    if api_key.is_empty() {
                        return Err(ApiError::Auth(format!(
                            "Missing credentials for provider: {}",
                            config.provider_name
                        )));
                    }

                    let base_url = key_rotation::key_at_index(config.base_url_env, key_index)
                        .unwrap_or_else(|| openai_compat::read_base_url(config));

                    let client = OpenAiCompatClient::new(api_key, config).with_base_url(base_url);
                    Ok(Self::OpenAi(client))
                }
            }
        }
    }

    #[must_use]
    pub const fn provider_kind(&self) -> ProviderKind {
        match self {
            Self::Anthropic(_) => ProviderKind::Anthropic,
            Self::Xai(_) => ProviderKind::Xai,
            Self::OpenAi(_) => ProviderKind::OpenAi,
        }
    }

    /// Escalate the δ breaker for a stream the transport accepted but that
    /// never delivered an event.
    ///
    /// `send_with_retry` records a breaker success as soon as the response is
    /// accepted, so without this a provider that opens a socket and then goes
    /// silent would de-escalate the breaker on every attempt.
    pub fn record_stream_stall(&self) {
        match self {
            Self::Anthropic(client) => client.record_stream_stall(),
            Self::Xai(client) | Self::OpenAi(client) => client.record_stream_stall(),
        }
    }

    #[must_use]
    pub fn with_prompt_cache(self, prompt_cache: PromptCache) -> Self {
        match self {
            Self::Anthropic(client) => Self::Anthropic(client.with_prompt_cache(prompt_cache)),
            other => other,
        }
    }

    #[must_use]
    pub fn prompt_cache_stats(&self) -> Option<PromptCacheStats> {
        match self {
            Self::Anthropic(client) => client.prompt_cache_stats(),
            Self::Xai(_) | Self::OpenAi(_) => None,
        }
    }

    #[must_use]
    pub fn take_last_prompt_cache_record(&self) -> Option<PromptCacheRecord> {
        match self {
            Self::Anthropic(client) => client.take_last_prompt_cache_record(),
            Self::Xai(_) | Self::OpenAi(_) => None,
        }
    }

    pub(crate) async fn apply_api_pause(
        model: &str,
        estimated_tokens: usize,
    ) -> Option<ApiLockGuard> {
        let pacing_started = Instant::now();
        let home = std::env::var("HOME").unwrap_or_default();
        let mut narration_wait = Duration::ZERO;
        if !home.is_empty() {
            let lock_path = std::path::Path::new(&home).join(".claw/narration.lock");
            let max_wait_secs: u64 = std::env::var("CLAW_TTS_WAIT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30);
            let start = std::time::Instant::now();
            // Give background voice narrator a brief window to detect new session entry and touch lock
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            while lock_path.exists() {
                if start.elapsed().as_secs() > max_wait_secs {
                    let _ = std::fs::remove_file(&lock_path);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            narration_wait = start.elapsed();
        }

        let interval_wait = apply_minimum_interval().await;

        // Apply model-specific TPM rate limiting
        let mut limit = if model.contains("glm") {
            20_000
        } else if model.contains("gemini") || model.contains("stable") {
            250_000
        } else if model.contains("mistral") || model.contains("codestral") {
            200_000
        } else {
            100_000
        };

        if model.contains("glm") {
            if let Ok(val) = std::env::var("CLAW_GLM_TPM_LIMIT") {
                if let Ok(parsed) = val.parse::<usize>() {
                    limit = parsed;
                }
            }
        } else if model.contains("gemini") || model.contains("stable") {
            if let Ok(val) = std::env::var("CLAW_GEMINI_TPM_LIMIT") {
                if let Ok(parsed) = val.parse::<usize>() {
                    limit = parsed;
                }
            }
        } else if let Ok(val) = std::env::var("CLAW_DEFAULT_TPM_LIMIT") {
            if let Ok(parsed) = val.parse::<usize>() {
                limit = parsed;
            }
        }

        if let Ok(val) = std::env::var("CLAW_TPM_LIMIT") {
            if let Ok(parsed) = val.parse::<usize>() {
                limit = parsed;
            }
        }

        let mut tpm_wait = Duration::ZERO;
        if limit > 0 {
            let limiter = if model.contains("glm") {
                GLM_LIMITER.get_or_init(TpmRateLimiter::new)
            } else if model.contains("gemini") || model.contains("stable") {
                GEMINI_LIMITER.get_or_init(TpmRateLimiter::new)
            } else {
                DEFAULT_LIMITER.get_or_init(TpmRateLimiter::new)
            };

            tpm_wait = limiter.acquire(limit, estimated_tokens).await;
        }

        // One line per request, split by cause: a slow turn is otherwise
        // indistinguishable from a slow provider, and the three waits below have
        // three completely different fixes.
        tracing::info!(
            model = %model,
            estimated_tokens,
            tpm_limit = limit,
            narration_wait_ms = narration_wait.as_millis() as u64,
            interval_wait_ms = interval_wait.as_millis() as u64,
            tpm_wait_ms = tpm_wait.as_millis() as u64,
            total_pacing_ms = pacing_started.elapsed().as_millis() as u64,
            "api pacing"
        );

        if !home.is_empty() {
            Some(ApiLockGuard::new(&home))
        } else {
            None
        }
    }

    pub async fn send_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse, ApiError> {
        let estimated_tokens = estimate_request_tokens(request);
        let _lock = Self::apply_api_pause(&request.model, estimated_tokens).await;

        let msg_count = request.messages.len();
        eprintln!(
            "📊 Request context: ~{} tokens estimated, {} messages",
            estimated_tokens, msg_count
        );

        let model = request.model.as_str();
        let mut key_index = get_active_key_index(model);
        if !Self::has_key_for_index(model, key_index) {
            key_index = 1;
        }
        let start_index = key_index;
        let total_keys = Self::available_key_count(model);
        let mut overload_fail_count: usize = 0;
        // Assigned by every `break` below; the success path returns early.
        let mut last_error: Option<ApiError>;
        let mut context_window_params: Option<(String, u32, u32, u32, u32)> = None;

        loop {
            if key_index != start_index {
                eprintln!("\n🔄 Using model '{model}' with rotated API key index {key_index}...");
            }

            let mut rotated_request = request.clone();
            rotated_request.inject_taskgraph_description();

            let client_res = if key_index == start_index {
                match self {
                    Self::Anthropic(client) => client.send_message(&rotated_request).await,
                    Self::Xai(client) | Self::OpenAi(client) => {
                        client.send_message(&rotated_request).await
                    }
                }
            } else if let Ok(fallback_client) =
                ProviderClient::from_model_with_key_index(model, key_index)
            {
                match fallback_client {
                    ProviderClient::Anthropic(client) => {
                        client.send_message(&rotated_request).await
                    }
                    ProviderClient::Xai(client) | ProviderClient::OpenAi(client) => {
                        client.send_message(&rotated_request).await
                    }
                }
            } else {
                Err(ApiError::Auth(format!(
                    "Missing credentials for model: {model}"
                )))
            };

            match client_res {
                Ok(response) => {
                    set_active_key_index(model, key_index);
                    return Ok(response);
                }
                Err(err) => {
                    eprintln!(
                        "⚠️ Model '{model}' with API key index {key_index} returned error: {err}."
                    );
                    Self::capture_context_window(&mut context_window_params, &err);

                    let err_str = format!("{err:?}");
                    let is_rate_limit = err.is_rate_limit()
                        || err_str.contains("1305")
                        || err_str.contains("429")
                        || err_str.contains("Too Many Requests")
                        || err_str.contains("overloaded");

                    last_error = Some(err);

                    if !is_rate_limit {
                        break;
                    }

                    overload_fail_count += 1;
                    // All keys exhausted with the same overload/1305 error —
                    // this is almost certainly a context overflow, not a rate
                    // limit.
                    if overload_fail_count >= total_keys && total_keys > 1 {
                        eprintln!(
                            "\n🚫 All {total_keys} API keys for model '{model}' returned overload/1305 errors."
                        );
                        eprintln!(
                            "   📊 Estimated context: ~{estimated_tokens} tokens, {msg_count} messages."
                        );
                        eprintln!(
                            "   💡 This is likely a context window overflow, not a rate limit."
                        );
                        eprintln!(
                            "   💡 Try: /clear to reset conversation, or /compact to reduce context size."
                        );
                        break;
                    }

                    // Every rotated client shares this model's δ breaker, and
                    // each chain already waits δ^L inside `gate_request`. A
                    // second ramp here would only double the pause.
                    eprintln!(
                        "⏳ Rate limit detected. Rotating key; the δ breaker paces the next attempt."
                    );
                    key_index += 1;
                    if !Self::has_key_for_index(model, key_index) {
                        key_index = 1;
                    }
                    set_active_key_index(model, key_index);

                    if key_index == start_index {
                        break;
                    }
                }
            }
        }

        Err(Self::rotation_error(
            model,
            context_window_params,
            last_error,
        ))
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        let estimated_tokens = estimate_request_tokens(request);
        let _lock = Self::apply_api_pause(&request.model, estimated_tokens).await;

        let msg_count = request.messages.len();
        tracing::debug!(
            estimated_tokens,
            messages = msg_count,
            model = %request.model,
            "stream context"
        );

        let model = request.model.as_str();
        let mut key_index = get_active_key_index(model);
        if !Self::has_key_for_index(model, key_index) {
            key_index = 1;
        }
        let start_index = key_index;
        let total_keys = Self::available_key_count(model);
        let mut overload_fail_count: usize = 0;
        // Assigned by every `break` below; the success path returns early.
        let mut last_error: Option<ApiError>;
        let mut context_window_params: Option<(String, u32, u32, u32, u32)> = None;

        loop {
            if key_index != start_index {
                eprintln!("\n🔄 Using model '{model}' with rotated API key index {key_index}...");
            }

            let mut rotated_request = request.clone();
            rotated_request.inject_taskgraph_description();

            let client_res = if key_index == start_index {
                match self {
                    Self::Anthropic(client) => client
                        .stream_message(&rotated_request)
                        .await
                        .map(MessageStream::Anthropic),
                    Self::Xai(client) | Self::OpenAi(client) => client
                        .stream_message(&rotated_request)
                        .await
                        .map(MessageStream::OpenAiCompat),
                }
            } else if let Ok(fallback_client) =
                ProviderClient::from_model_with_key_index(model, key_index)
            {
                match fallback_client {
                    ProviderClient::Anthropic(client) => client
                        .stream_message(&rotated_request)
                        .await
                        .map(MessageStream::Anthropic),
                    ProviderClient::Xai(client) | ProviderClient::OpenAi(client) => client
                        .stream_message(&rotated_request)
                        .await
                        .map(MessageStream::OpenAiCompat),
                }
            } else {
                Err(ApiError::Auth(format!(
                    "Missing credentials for model: {model}"
                )))
            };

            match client_res {
                Ok(response) => {
                    set_active_key_index(model, key_index);
                    return Ok(response);
                }
                Err(err) => {
                    eprintln!(
                        "⚠️ Model '{model}' with API key index {key_index} returned error in streaming: {err}."
                    );
                    Self::capture_context_window(&mut context_window_params, &err);

                    let status_code = err.status_code().unwrap_or(0);
                    let err_str = format!("{err:?}");
                    // A transport/connection failure (no HTTP response) is the
                    // endpoint's problem, not this key's: every key in the pool
                    // hits the same host, so cooling this one and rotating would
                    // only poison healthy siblings. Let it fall through to the
                    // provider-wide backoff ladder untouched.
                    let is_cooldown_error =
                        crate::cooldown::KeyCooldownTracker::should_trigger_cooldown(
                            &err_str,
                            status_code,
                        ) && !crate::cooldown::KeyCooldownTracker::is_endpoint_or_network_error(
                            &err_str,
                            status_code,
                        );

                    last_error = Some(err);

                    if !is_cooldown_error {
                        break;
                    }

                    overload_fail_count += 1;
                    crate::cooldown::GLOBAL_KEY_COOLDOWN.mark_cooldown(
                        model,
                        key_index,
                        crate::cooldown::DEFAULT_COOLDOWN_DURATION,
                    );

                    let (next_key, min_wait) = crate::cooldown::GLOBAL_KEY_COOLDOWN
                        .find_available_key(model, total_keys, key_index + 1);

                    if let Some(wait_time) = min_wait {
                        if overload_fail_count >= total_keys && total_keys > 1 {
                            eprintln!(
                                "\n🚫 All {total_keys} API keys for model '{model}' are in cooldown or overloaded."
                            );
                            eprintln!(
                                "   📊 Estimated context: ~{estimated_tokens} tokens, {msg_count} messages."
                            );
                            eprintln!(
                                "   💡 Try: /clear to reset conversation, or /compact to reduce context size."
                            );
                            break;
                        }

                        let wait_secs = std::cmp::min(wait_time.as_secs(), 15).max(2);
                        eprintln!(
                            "⏳ All API keys for '{model}' in cooldown. Waiting {wait_secs}s for key {next_key} to recover..."
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
                    } else {
                        eprintln!(
                            "❄️ API key {key_index} placed in 60s cooldown. Switching to available key {next_key}..."
                        );
                    }

                    key_index = next_key;
                    set_active_key_index(model, key_index);

                    if key_index == start_index && min_wait.is_none() {
                        break;
                    }
                }
            }
        }

        Err(Self::rotation_error(
            model,
            context_window_params,
            last_error,
        ))
    }

    /// Credential slots available for a model (`KEY`, `KEY_2`, … ).
    fn available_key_count(model: &str) -> usize {
        (1usize..)
            .take_while(|index| Self::has_key_for_index(model, *index))
            .count()
    }

    /// Remember the first context-window overflow seen during key rotation;
    /// later overloads are noise next to a measured overflow.
    fn capture_context_window(params: &mut Option<(String, u32, u32, u32, u32)>, error: &ApiError) {
        if let ApiError::ContextWindowExceeded {
            model,
            estimated_input_tokens,
            requested_output_tokens,
            estimated_total_tokens,
            context_window_tokens,
        } = error
        {
            if params.is_none() {
                *params = Some((
                    model.clone(),
                    *estimated_input_tokens,
                    *requested_output_tokens,
                    *estimated_total_tokens,
                    *context_window_tokens,
                ));
            }
        }
    }

    /// Error a fully rotated request ends with: a captured overflow outranks
    /// the last transport error, since overloads often mask an overflow.
    fn rotation_error(
        requested_model: &str,
        context_window_params: Option<(String, u32, u32, u32, u32)>,
        last_error: Option<ApiError>,
    ) -> ApiError {
        if let Some((
            model,
            estimated_input_tokens,
            requested_output_tokens,
            estimated_total_tokens,
            context_window_tokens,
        )) = context_window_params
        {
            return ApiError::ContextWindowExceeded {
                model,
                estimated_input_tokens,
                requested_output_tokens,
                estimated_total_tokens,
                context_window_tokens,
            };
        }

        last_error.unwrap_or_else(|| {
            ApiError::Auth(format!("Missing credentials for model: {requested_model}"))
        })
    }
}

#[derive(Debug)]
pub enum MessageStream {
    Anthropic(anthropic::MessageStream),
    OpenAiCompat(openai_compat::MessageStream),
}

impl MessageStream {
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Anthropic(stream) => stream.request_id(),
            Self::OpenAiCompat(stream) => stream.request_id(),
        }
    }

    pub async fn next_event(&mut self) -> Result<Option<StreamEvent>, ApiError> {
        match self {
            Self::Anthropic(stream) => stream.next_event().await,
            Self::OpenAiCompat(stream) => stream.next_event().await,
        }
    }
}

pub use anthropic::{
    oauth_token_is_expired, resolve_saved_oauth_token, resolve_startup_auth_source, OAuthTokenSet,
};
#[must_use]
pub fn read_base_url() -> String {
    anthropic::read_base_url()
}

#[must_use]
pub fn read_xai_base_url() -> String {
    openai_compat::read_base_url(OpenAiCompatConfig::xai())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use super::ProviderClient;
    use crate::providers::{detect_provider_kind, resolve_model_alias, ProviderKind};

    /// Serializes every test in this module that mutates process-wide
    /// environment variables so concurrent test threads cannot observe
    /// each other's partially-applied state.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn resolves_existing_and_grok_aliases() {
        assert_eq!(resolve_model_alias("opus"), "claude-opus-4-7");
        assert_eq!(resolve_model_alias("grok"), "grok-3");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-3-mini");
    }

    #[test]
    fn provider_detection_prefers_model_family() {
        let _lock = env_lock();
        let _ollama = EnvVarGuard::set("OLLAMA_HOST", None);
        assert_eq!(detect_provider_kind("grok-3"), ProviderKind::Xai);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::Anthropic
        );
    }

    /// Snapshot-restore guard for a single environment variable. Mirrors
    /// the pattern used in `providers/mod.rs` tests: captures the original
    /// value on construction, applies the override, and restores on drop so
    /// tests leave the process env untouched even when they panic.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let original = std::env::var_os(key);
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn dashscope_model_uses_dashscope_config_not_openai() {
        // Regression: qwen-plus was being routed to OpenAiCompatConfig::openai()
        // which reads OPENAI_API_KEY and points at api.openai.com, when it should
        // use OpenAiCompatConfig::dashscope() which reads DASHSCOPE_API_KEY and
        // points at dashscope.aliyuncs.com.
        let _lock = env_lock();
        let _ollama = EnvVarGuard::set("OLLAMA_HOST", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", Some("test-dashscope-key"));
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);

        let client = ProviderClient::from_model("qwen-plus");

        // Must succeed (not fail with "missing OPENAI_API_KEY")
        assert!(
            client.is_ok(),
            "qwen-plus with DASHSCOPE_API_KEY set should build successfully, got: {:?}",
            client.err()
        );

        // Verify it's the OpenAi variant pointed at the DashScope base URL.
        match client.unwrap() {
            ProviderClient::OpenAi(openai_client) => {
                assert!(
                    openai_client.base_url().contains("dashscope.aliyuncs.com"),
                    "qwen-plus should route to DashScope base URL (contains 'dashscope.aliyuncs.com'), got: {}",
                    openai_client.base_url()
                );
            }
            other => panic!("Expected ProviderClient::OpenAi for qwen-plus, got: {other:?}"),
        }
    }

    #[test]
    fn local_openai_base_url_routes_authless_ollama_models() {
        let _lock = env_lock();
        let _ollama = EnvVarGuard::set("OLLAMA_HOST", None);
        let _base_url = EnvVarGuard::set("OPENAI_BASE_URL", Some("http://127.0.0.1:11434/v1"));
        let _openai_key = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _anthropic_key = EnvVarGuard::set("ANTHROPIC_API_KEY", Some("test-anthropic-key"));
        let _anthropic_token = EnvVarGuard::set("ANTHROPIC_AUTH_TOKEN", None);

        let client = ProviderClient::from_model("qwen2.5-coder:7b")
            .expect("local model should route to OpenAI-compatible client without auth");
        match client {
            ProviderClient::OpenAi(openai_client) => {
                assert_eq!(openai_client.base_url(), "http://127.0.0.1:11434/v1")
            }
            other => panic!("Expected ProviderClient::OpenAi for local model, got: {other:?}"),
        }
    }

    #[test]
    fn minimum_interval_parses_only_positive_seconds() {
        use super::minimum_interval_from;
        assert_eq!(
            minimum_interval_from(Some("5")),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            minimum_interval_from(Some(" 5 ")),
            Some(Duration::from_secs(5))
        );
        assert_eq!(minimum_interval_from(Some("0")), None);
        assert_eq!(minimum_interval_from(Some("-1")), None);
        assert_eq!(minimum_interval_from(Some("soon")), None);
        assert_eq!(minimum_interval_from(None), None);
    }

    #[test]
    fn tpm_wait_is_zero_while_the_budget_fits() {
        use super::minimal_wait;
        let now = Instant::now();
        let entries = [(now - Duration::from_secs(10), 4_000usize)];
        assert_eq!(
            minimal_wait(&entries, 4_000, 20_000, 5_000, now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn tpm_wait_ends_when_enough_budget_frees_not_when_the_oldest_entry_does() {
        use super::minimal_wait;
        let now = Instant::now();
        // Recorded oldest-first, but deliberately passed out of order: the wait
        // must be driven by expiry time, not by insertion order.
        let entries = [
            (now - Duration::from_secs(30), 6_000usize), // frees at +30 s
            (now - Duration::from_secs(50), 5_000usize), // frees at +10 s
            (now - Duration::from_secs(40), 4_000usize), // frees at +20 s
        ];
        // Budget 20 000, 15 000 used, 8 000 requested → must free 3 000.
        // The +10 s expiry alone frees 5 000, which is enough.
        let wait =
            minimal_wait(&entries, 15_000, 20_000, 8_000, now).expect("a finite wait must exist");
        assert_eq!(wait, Duration::from_secs(10), "waited for the wrong expiry");
    }

    #[test]
    fn tpm_wait_accumulates_expiries_until_the_request_fits() {
        use super::minimal_wait;
        let now = Instant::now();
        let entries = [
            (now - Duration::from_secs(50), 1_000usize),
            (now - Duration::from_secs(45), 1_000usize),
            (now - Duration::from_secs(40), 5_000usize),
        ];
        // 7 000 used of a 10 000 budget, 6 000 requested → must free 3 000,
        // which only the third expiry provides.
        let wait =
            minimal_wait(&entries, 7_000, 10_000, 6_000, now).expect("a finite wait must exist");
        assert_eq!(wait, Duration::from_secs(20));
    }

    #[test]
    fn tpm_wait_is_none_when_the_window_cannot_free_enough() {
        use super::minimal_wait;
        let now = Instant::now();
        let entries = [(now - Duration::from_secs(5), 1_000usize)];
        assert_eq!(minimal_wait(&entries, 1_000, 2_000, 5_000, now), None);
    }

    #[tokio::test]
    async fn a_request_larger_than_the_whole_budget_is_admitted_immediately() {
        use super::TpmRateLimiter;
        let limiter = TpmRateLimiter::new();
        let started = Instant::now();
        // 28 943 estimated tokens against the real glm limit of 20 000: the
        // shape that produced 14–43 s stalls in production.
        let waited = limiter.acquire(20_000, 28_943).await;
        assert_eq!(waited, Duration::ZERO);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "oversized request should not drain the window, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_request_that_fits_is_admitted_without_waiting() {
        use super::TpmRateLimiter;
        let limiter = TpmRateLimiter::new();
        let started = Instant::now();
        let waited = limiter.acquire(100_000, 5_000).await;
        assert_eq!(waited, Duration::ZERO);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    // The guard has to outlive every await below: this test measures wall-clock
    // spacing, so releasing the env lock mid-sequence would let another test
    // rewrite CLAW_API_PAUSE_SECS and corrupt the measurement it is guarding.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn the_api_pause_is_a_floor_on_spacing_not_a_tax_on_every_request() {
        let _lock = env_lock();
        // An isolated HOME so a live voice narrator's `narration.lock` cannot
        // make this test wait on TTS.
        let home = std::env::temp_dir().join(format!("claw-pacing-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("create temp home");
        let _home_guard = EnvVarGuard::set("HOME", home.to_str());
        let _pause = EnvVarGuard::set("CLAW_API_PAUSE_SECS", Some("1"));
        let _tpm = EnvVarGuard::set("CLAW_TPM_LIMIT", Some("100000000"));
        super::reset_minimum_interval();

        let first = Instant::now();
        ProviderClient::apply_api_pause("test-model", 100).await;
        assert!(
            first.elapsed() < Duration::from_millis(500),
            "the first request has no predecessor to space itself from, took {:?}",
            first.elapsed()
        );

        let second = Instant::now();
        ProviderClient::apply_api_pause("test-model", 100).await;
        assert!(
            second.elapsed() >= Duration::from_millis(500),
            "back-to-back requests must respect the floor, waited {:?}",
            second.elapsed()
        );

        // Work that already took longer than the floor pays nothing: this is the
        // 362 s of dead time the unconditional sleep used to add per turn.
        tokio::time::sleep(Duration::from_millis(1_200)).await;
        let third = Instant::now();
        ProviderClient::apply_api_pause("test-model", 100).await;
        assert!(
            third.elapsed() < Duration::from_millis(300),
            "elapsed work must count toward the floor, waited {:?}",
            third.elapsed()
        );

        let _ = std::fs::remove_dir_all(&home);
    }
}
