use std::sync::{Mutex, OnceLock, RwLock};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use crate::error::ApiError;
use crate::key_rotation;
use crate::prompt_cache::{PromptCache, PromptCacheRecord, PromptCacheStats};
use crate::providers::anthropic::{self, AnthropicClient, AuthSource};
use crate::providers::openai_compat::{self, OpenAiCompatClient, OpenAiCompatConfig};
use crate::providers::{self, ProviderKind};
use crate::types::{MessageRequest, MessageResponse, StreamEvent};

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

    async fn acquire(&self, limit: usize, estimated_tokens: usize) {
        loop {
            let now = Instant::now();
            let wait_duration = {
                let mut lock = match self.window.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };

                lock.retain(|(time, _)| now.duration_since(*time) < Duration::from_secs(60));

                let current_tokens: usize = lock.iter().map(|(_, tokens)| tokens).sum();

                if current_tokens + estimated_tokens <= limit {
                    lock.push((now, estimated_tokens));
                    return;
                }

                if let Some(&(first_time, _)) = lock.first() {
                    let elapsed = now.duration_since(first_time);
                    Duration::from_secs(60).saturating_sub(elapsed)
                } else {
                    lock.push((now, estimated_tokens));
                    return;
                }
            };

            tokio::time::sleep(wait_duration).await;
        }
    }
}

static GLM_LIMITER: OnceLock<TpmRateLimiter> = OnceLock::new();
static GEMINI_LIMITER: OnceLock<TpmRateLimiter> = OnceLock::new();
static DEFAULT_LIMITER: OnceLock<TpmRateLimiter> = OnceLock::new();

fn estimate_request_tokens(request: &MessageRequest) -> usize {
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

struct ApiLockGuard {
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
        if let Some(auth) = anthropic_auth {
            return Ok(Self::Anthropic(AnthropicClient::from_auth(auth)));
        }
        let resolved_model = providers::resolve_model_alias(model);
        match providers::detect_provider_kind(&resolved_model) {
            ProviderKind::Anthropic => Ok(Self::Anthropic(AnthropicClient::from_env()?)),
            ProviderKind::Xai => Ok(Self::Xai(OpenAiCompatClient::from_env(
                OpenAiCompatConfig::xai(),
            )?)),
            ProviderKind::OpenAi => {
                // OLLAMA_HOST takes priority: local Ollama needs no API key
                // and ignores DashScope/OpenAI env-based dispatch.
                if std::env::var_os("OLLAMA_HOST").is_some() {
                    Ok(Self::OpenAi(
                        openai_compat::OpenAiCompatClient::from_ollama_env()
                            .expect("from_ollama_env always returns Some"),
                    ))
                } else {
                    // DashScope models (qwen-*) also return ProviderKind::OpenAi because they
                    // speak the OpenAI wire format, but they need the DashScope config which
                    // reads DASHSCOPE_API_KEY and points at dashscope.aliyuncs.com.
                    let config = match providers::metadata_for_model(&resolved_model) {
                        Some(meta) if meta.auth_env == "DASHSCOPE_API_KEY" => {
                            OpenAiCompatConfig::dashscope()
                        }
                        Some(meta) if meta.auth_env == "GLM_API_KEY" => OpenAiCompatConfig::glm(),
                        Some(meta) if meta.auth_env == "CLOUDFLARE_API_TOKEN" => OpenAiCompatConfig::cloudflare(),
                        Some(meta) if meta.auth_env == "NVIDIA_API_KEY" => OpenAiCompatConfig::nvidia(),
                        Some(meta) if meta.auth_env == "GEMINI_API_KEY" => OpenAiCompatConfig::gemini(),
                        Some(meta) if meta.auth_env == "SILICONFLOW_API_KEY" => OpenAiCompatConfig::siliconflow(),
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
            ProviderKind::OpenAi => {
                match providers::metadata_for_model(&resolved_model) {
                    Some(meta) => meta.auth_env,
                    None => "OPENAI_API_KEY",
                }
            }
        };
        key_rotation::has_key_at_index(api_key_env, key_index)
    }

    pub fn from_model_with_key_index(
        model: &str,
        key_index: usize,
    ) -> Result<Self, ApiError> {
        let resolved_model = providers::resolve_model_alias(model);
        match providers::detect_provider_kind(&resolved_model) {
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
                    (None, None) => return Err(ApiError::Auth("Missing ANTHROPIC_API_KEY".to_string())),
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
                let api_key = key_rotation::key_at_index(config.api_key_env, key_index)
                    .unwrap_or_default();
                if api_key.is_empty() {
                    return Err(ApiError::Auth(format!("Missing credentials for provider: {}", config.provider_name)));
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
                if std::env::var_os("OLLAMA_HOST").is_some() {
                    Ok(Self::OpenAi(
                        openai_compat::OpenAiCompatClient::from_ollama_env()
                            .expect("from_ollama_env always returns Some"),
                    ))
                } else {
                    let config = match providers::metadata_for_model(&resolved_model) {
                        Some(meta) if meta.auth_env == "DASHSCOPE_API_KEY" => {
                            OpenAiCompatConfig::dashscope()
                        }
                        Some(meta) if meta.auth_env == "GLM_API_KEY" => OpenAiCompatConfig::glm(),
                        Some(meta) if meta.auth_env == "CLOUDFLARE_API_TOKEN" => OpenAiCompatConfig::cloudflare(),
                        Some(meta) if meta.auth_env == "NVIDIA_API_KEY" => OpenAiCompatConfig::nvidia(),
                        Some(meta) if meta.auth_env == "GEMINI_API_KEY" => OpenAiCompatConfig::gemini(),
                        Some(meta) if meta.auth_env == "SILICONFLOW_API_KEY" => OpenAiCompatConfig::siliconflow(),
                        _ => OpenAiCompatConfig::openai(),
                    };
                    
                    let api_key = key_rotation::key_at_index(config.api_key_env, key_index)
                        .unwrap_or_default();
                    
                    if api_key.is_empty() {
                        return Err(ApiError::Auth(format!("Missing credentials for provider: {}", config.provider_name)));
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


async fn apply_api_pause(model: &str, estimated_tokens: usize) -> Option<ApiLockGuard> {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        let lock_path = std::path::Path::new(&home).join(".claw/narration.lock");
        let start = std::time::Instant::now();
        while lock_path.exists() {
            if start.elapsed().as_secs() > 5 {
                let _ = std::fs::remove_file(&lock_path);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    if let Ok(val) = std::env::var("CLAW_API_PAUSE_SECS") {
        if let Ok(secs) = val.parse::<u64>() {
            if secs > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            }
        }
    }

    // Default rate limit sleep of 1 second between requests
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Apply model-specific TPM rate limiting
    let mut limit = if model.contains("glm") {
        20_000
    } else if model.contains("gemini") || model.contains("stable") {
        250_000
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

    if limit > 0 {
        let limiter = if model.contains("glm") {
            GLM_LIMITER.get_or_init(TpmRateLimiter::new)
        } else if model.contains("gemini") || model.contains("stable") {
            GEMINI_LIMITER.get_or_init(TpmRateLimiter::new)
        } else {
            DEFAULT_LIMITER.get_or_init(TpmRateLimiter::new)
        };

        limiter.acquire(limit, estimated_tokens).await;
    }

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
        
        let models_to_try = vec![request.model.clone()];
        
        let mut last_error = None;
        
        for model in &models_to_try {
            let mut key_index = get_active_key_index(model);
            if !Self::has_key_for_index(model, key_index) {
                key_index = 1;
            }
            let start_index = key_index;
            let mut overload_fail_count: usize = 0;
            let mut total_keys: usize = 0;
            
            // Count total available keys for this model
            {
                let mut k = 1;
                while Self::has_key_for_index(model, k) {
                    total_keys += 1;
                    k += 1;
                }
            }
            
            let mut attempt: u32 = 0;
            
            loop {
                if key_index != start_index || model != &request.model {
                    eprintln!(
                        "\n⚠️ Switching to model '{}' with API key index {}...",
                        model, key_index
                    );
                }
                
                let mut fallback_request = request.clone();
                fallback_request.model = model.clone();
                fallback_request.inject_taskgraph_description();
                
                let client_res = if key_index == start_index && model == &request.model {
                    match self {
                        Self::Anthropic(client) => client.send_message(&fallback_request).await,
                        Self::Xai(client) | Self::OpenAi(client) => client.send_message(&fallback_request).await,
                    }
                } else if let Ok(fallback_client) = ProviderClient::from_model_with_key_index(model, key_index) {
                    match fallback_client {
                        ProviderClient::Anthropic(client) => client.send_message(&fallback_request).await,
                        ProviderClient::Xai(client) | ProviderClient::OpenAi(client) => client.send_message(&fallback_request).await,
                    }
                } else {
                    Err(ApiError::Auth(format!("Missing credentials for model: {}", model)))
                };
                
                match client_res {
                    Ok(response) => {
                        set_active_key_index(model, key_index);
                        return Ok(response);
                    }
                    Err(err) => {
                        eprintln!(
                            "⚠️ Model '{}' with API key index {} returned error: {}.",
                            model, key_index, err
                        );
                        
                        let err_str = format!("{:?}", err);
                        let is_rate_limit = err.is_rate_limit() 
                            || err_str.contains("1305") 
                            || err_str.contains("429")
                            || err_str.contains("Too Many Requests")
                            || err_str.contains("overloaded");
                        
                        if is_rate_limit {
                            overload_fail_count += 1;
                        }
                        
                        last_error = Some(err);
                        
                        if is_rate_limit {
                            // All keys exhausted with the same overload/1305 error — 
                            // this is almost certainly a context overflow, not a rate limit
                            if overload_fail_count >= total_keys && total_keys > 1 {
                                eprintln!(
                                    "\n🚫 All {} API keys for model '{}' returned overload/1305 errors.",
                                    total_keys, model
                                );
                                eprintln!(
                                    "   📊 Estimated context: ~{} tokens, {} messages.",
                                    estimated_tokens, msg_count
                                );
                                eprintln!(
                                    "   💡 This is likely a context window overflow, not a rate limit."
                                );
                                eprintln!(
                                    "   💡 Try: /clear to reset conversation, or /compact to reduce context size."
                                );
                                break;
                            }
                            
                            attempt += 1;
                            let backoff_secs = std::cmp::min(2u64.pow(attempt), 16);
                            eprintln!(
                                "⏳ Rate limit detected. Pausing for {} seconds before retry...",
                                backoff_secs
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            
                            key_index += 1;
                            if !Self::has_key_for_index(model, key_index) {
                                key_index = 1;
                            }
                            set_active_key_index(model, key_index);
                            
                            if key_index == start_index {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| ApiError::Auth(format!("Missing credentials for model: {}", request.model))))
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        let estimated_tokens = estimate_request_tokens(request);
        let _lock = Self::apply_api_pause(&request.model, estimated_tokens).await;
        
        let msg_count = request.messages.len();
        eprintln!(
            "📊 Stream context: ~{} tokens estimated, {} messages",
            estimated_tokens, msg_count
        );
        
        let models_to_try = build_fallback_model_cascade(&request.model);
        
        let mut last_error = None;
        for model in &models_to_try {
            let mut key_index = get_active_key_index(model);
            if !Self::has_key_for_index(model, key_index) {
                key_index = 1;
            }
            let start_index = key_index;
            let mut overload_fail_count: usize = 0;
            let mut total_keys: usize = 0;
            
            // Count total available keys for this model
            {
                let mut k = 1;
                while Self::has_key_for_index(model, k) {
                    total_keys += 1;
                    k += 1;
                }
            }
            
            loop {
                if key_index != start_index || model != &request.model {
                    eprintln!(
                        "\n⚠️ Switching to model '{}' with API key index {}...",
                        model, key_index
                    );
                }
                
                let mut fallback_request = request.clone();
                fallback_request.model = model.clone();
                fallback_request.inject_taskgraph_description();
                
                let client_res = if key_index == start_index && model == &request.model {
                    match self {
                        Self::Anthropic(client) => client
                            .stream_message(&fallback_request)
                            .await
                            .map(MessageStream::Anthropic),
                        Self::Xai(client) | Self::OpenAi(client) => client
                            .stream_message(&fallback_request)
                            .await
                            .map(MessageStream::OpenAiCompat),
                    }
                } else if let Ok(fallback_client) = ProviderClient::from_model_with_key_index(model, key_index) {
                    match fallback_client {
                        ProviderClient::Anthropic(client) => client
                            .stream_message(&fallback_request)
                            .await
                            .map(MessageStream::Anthropic),
                        ProviderClient::Xai(client) | ProviderClient::OpenAi(client) => client
                            .stream_message(&fallback_request)
                            .await
                            .map(MessageStream::OpenAiCompat),
                    }
                } else {
                    Err(ApiError::Auth(format!("Missing credentials for model: {}", model)))
                };
                
                match client_res {
                    Ok(response) => {
                        set_active_key_index(model, key_index);
                        return Ok(response);
                    }
                    Err(err) => {
                        eprintln!(
                            "⚠️ Model '{}' with API key index {} returned error in streaming: {}.",
                            model, key_index, err
                        );
                        
                        let status_code = err.status_code().unwrap_or(0);
                        let err_str = format!("{:?}", err);
                        let is_cooldown_error = crate::cooldown::KeyCooldownTracker::should_trigger_cooldown(&err_str, status_code);

                        last_error = Some(err);

                        if is_cooldown_error {
                            overload_fail_count += 1;
                            crate::cooldown::GLOBAL_KEY_COOLDOWN.mark_cooldown(
                                model,
                                key_index,
                                crate::cooldown::DEFAULT_COOLDOWN_DURATION,
                            );

                            let (next_key, min_wait) = crate::cooldown::GLOBAL_KEY_COOLDOWN.find_available_key(
                                model,
                                total_keys,
                                key_index + 1,
                            );

                            if let Some(wait_time) = min_wait {
                                if overload_fail_count >= total_keys && total_keys > 1 {
                                    eprintln!(
                                        "\n🚫 All {} API keys for model '{}' are in cooldown or overloaded.",
                                        total_keys, model
                                    );
                                    eprintln!(
                                        "   📊 Estimated context: ~{} tokens, {} messages.",
                                        estimated_tokens, msg_count
                                    );
                                    eprintln!(
                                        "   💡 Try: /clear to reset conversation, or /compact to reduce context size."
                                    );
                                    break;
                                }

                                let wait_secs = std::cmp::min(wait_time.as_secs(), 15).max(2);
                                eprintln!(
                                    "⏳ All API keys for '{}' in cooldown. Waiting {}s for key {} to recover...",
                                    model, wait_secs, next_key
                                );
                                tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
                            } else {
                                eprintln!(
                                    "❄️ API key {} placed in 60s cooldown. Switching to available key {}...",
                                    key_index, next_key
                                );
                            }

                            key_index = next_key;
                            set_active_key_index(model, key_index);

                            if key_index == start_index && min_wait.is_none() {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| ApiError::Auth(format!("Missing credentials for model: {}", request.model))))
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
    fn test_fallback_model_cascade_generation() {
        let cascade_glm = super::build_fallback_model_cascade("glm-4.7-flash");
        assert!(cascade_glm.len() >= 2);
        assert_eq!(cascade_glm[0], "glm-4.7-flash");
        assert!(cascade_glm.contains(&"gemini-3.1-flash-lite".to_string()));

        let cascade_gemini = super::build_fallback_model_cascade("gemini-3.5-flash");
        assert!(cascade_gemini.contains(&"glm-4.7-flash".to_string()));
    }
}

pub fn build_fallback_model_cascade(primary_model: &str) -> Vec<String> {
    let mut cascade = vec![primary_model.to_string()];
    let lower = primary_model.to_lowercase();
    if lower.contains("glm") || lower.contains("zhipu") {
        if !cascade.contains(&"gemini-3.1-flash-lite".to_string()) {
            cascade.push("gemini-3.1-flash-lite".to_string());
        }
        if !cascade.contains(&"quick".to_string()) {
            cascade.push("quick".to_string());
        }
    } else if lower.contains("gemini") {
        if !cascade.contains(&"glm-4.7-flash".to_string()) {
            cascade.push("glm-4.7-flash".to_string());
        }
        if !cascade.contains(&"quick".to_string()) {
            cascade.push("quick".to_string());
        }
    } else {
        if !cascade.contains(&"gemini-3.1-flash-lite".to_string()) {
            cascade.push("gemini-3.1-flash-lite".to_string());
        }
    }
    cascade
}
