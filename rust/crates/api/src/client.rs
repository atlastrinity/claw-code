use crate::error::ApiError;
use crate::prompt_cache::{PromptCache, PromptCacheRecord, PromptCacheStats};
use crate::providers::anthropic::{self, AnthropicClient, AuthSource};
use crate::providers::openai_compat::{self, OpenAiCompatClient, OpenAiCompatConfig};
use crate::providers::{self, ProviderKind};
use crate::types::{MessageRequest, MessageResponse, StreamEvent};

struct ApiLockGuard {
    lock_path: std::path::PathBuf,
}

impl ApiLockGuard {
    fn new(home: &str) -> Self {
        let lock_path = std::path::Path::new(home).join(".claw/api.lock");
        if let Some(parent) = lock_path.parent() {
            let _ = std::fs::create_dir_all(parent);
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
        Self::from_model_with_anthropic_auth(model, None)
    }

    pub fn from_model_with_anthropic_auth(
        model: &str,
        anthropic_auth: Option<AuthSource>,
    ) -> Result<Self, ApiError> {
        let resolved_model = providers::resolve_model_alias(model);
        match providers::detect_provider_kind(&resolved_model) {
            ProviderKind::Anthropic => Ok(Self::Anthropic(match anthropic_auth {
                Some(auth) => AnthropicClient::from_auth(auth),
                None => AnthropicClient::from_env()?,
            })),
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
                        _ => OpenAiCompatConfig::openai(),
                    };
                    Ok(Self::OpenAi(OpenAiCompatClient::from_env(config)?))
                }
            }
        }
    }

    pub fn has_key_for_index(model: &str, key_index: usize) -> bool {
        if key_index == 1 {
            return true;
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
        let key_var = format!("{}{}", api_key_env, key_index);
        if let Ok(val) = std::env::var(&key_var) {
            !val.trim().is_empty()
        } else {
            false
        }
    }

    pub fn from_model_with_key_index(
        model: &str,
        key_index: usize,
    ) -> Result<Self, ApiError> {
        let resolved_model = providers::resolve_model_alias(model);
        match providers::detect_provider_kind(&resolved_model) {
            ProviderKind::Anthropic => {
                let mut api_key_val = None;
                let mut auth_token_val = None;
                
                if key_index > 1 {
                    let key_var = format!("ANTHROPIC_API_KEY{}", key_index);
                    let token_var = format!("ANTHROPIC_AUTH_TOKEN{}", key_index);
                    if let Ok(val) = std::env::var(&key_var) {
                        if !val.trim().is_empty() {
                            api_key_val = Some(val);
                        }
                    }
                    if let Ok(val) = std::env::var(&token_var) {
                        if !val.trim().is_empty() {
                            auth_token_val = Some(val);
                        }
                    }
                } else {
                    if let Ok(val) = std::env::var("ANTHROPIC_API_KEY") {
                        if !val.trim().is_empty() {
                            api_key_val = Some(val);
                        }
                    }
                    if let Ok(val) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
                        if !val.trim().is_empty() {
                            auth_token_val = Some(val);
                        }
                    }
                }
                
                let auth_source = match (api_key_val, auth_token_val) {
                    (Some(api_key), Some(bearer_token)) => AuthSource::ApiKeyAndBearer {
                        api_key,
                        bearer_token,
                    },
                    (Some(api_key), None) => AuthSource::ApiKey(api_key),
                    (None, Some(bearer_token)) => AuthSource::BearerToken(bearer_token),
                    (None, None) => return Err(ApiError::Auth("Missing ANTHROPIC_API_KEY".to_string())),
                };
                
                let mut client = AnthropicClient::from_auth(auth_source);
                
                let base_url_env = if key_index > 1 {
                    format!("ANTHROPIC_BASE_URL{}", key_index)
                } else {
                    "ANTHROPIC_BASE_URL".to_string()
                };
                if let Ok(url) = std::env::var(&base_url_env) {
                    if !url.trim().is_empty() {
                        client = client.with_base_url(url);
                    }
                }
                
                Ok(Self::Anthropic(client))
            }
            ProviderKind::Xai => {
                let config = OpenAiCompatConfig::xai();
                let key_var = if key_index > 1 {
                    format!("{}{}", config.api_key_env, key_index)
                } else {
                    config.api_key_env.to_string()
                };
                let url_var = if key_index > 1 {
                    format!("{}{}", config.base_url_env, key_index)
                } else {
                    config.base_url_env.to_string()
                };
                
                let api_key = match std::env::var(&key_var) {
                    Ok(val) if !val.trim().is_empty() => val.trim().to_string(),
                    _ => std::env::var(config.api_key_env).unwrap_or_default()
                };
                if api_key.is_empty() {
                    return Err(ApiError::Auth(format!("Missing credentials for provider: {}", config.provider_name)));
                }
                let base_url = match std::env::var(&url_var) {
                    Ok(val) if !val.trim().is_empty() => val.trim().to_string(),
                    _ => std::env::var(config.base_url_env).unwrap_or_else(|_| config.default_base_url.to_string())
                };
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
                        _ => OpenAiCompatConfig::openai(),
                    };
                    
                    let key_var = if key_index > 1 {
                        format!("{}{}", config.api_key_env, key_index)
                    } else {
                        config.api_key_env.to_string()
                    };
                    
                    let url_var = if key_index > 1 {
                        format!("{}{}", config.base_url_env, key_index)
                    } else {
                        config.base_url_env.to_string()
                    };
                    
                    let api_key = match std::env::var(&key_var) {
                        Ok(val) if !val.trim().is_empty() => val.trim().to_string(),
                        _ => {
                            std::env::var(config.api_key_env).unwrap_or_default()
                        }
                    };
                    
                    if api_key.is_empty() {
                        return Err(ApiError::Auth(format!("Missing credentials for provider: {}", config.provider_name)));
                    }
                    
                    let base_url = match std::env::var(&url_var) {
                        Ok(val) if !val.trim().is_empty() => val.trim().to_string(),
                        _ => {
                            std::env::var(config.base_url_env).unwrap_or_else(|_| config.default_base_url.to_string())
                        }
                    };
                    
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


async fn apply_api_pause() -> Option<ApiLockGuard> {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        let lock_path = std::path::Path::new(&home).join(".claw/narration.lock");
        while lock_path.exists() {
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
        let _lock = Self::apply_api_pause().await;
        
        let mut models_to_try = vec![request.model.clone()];
        let stable_model = crate::providers::resolve_model_alias("stable");
        let gemini_lite_model = crate::providers::resolve_model_alias("gemini-lite");
        
        if !models_to_try.contains(&stable_model) {
            models_to_try.push(stable_model);
        }
        if !models_to_try.contains(&gemini_lite_model) {
            models_to_try.push(gemini_lite_model);
        }
        
        let mut last_error = None;
        
        for model in &models_to_try {
            let mut key_index = 1;
            while Self::has_key_for_index(model, key_index) {
                if key_index > 1 || model != &request.model {
                    eprintln!(
                        "\n⚠️ Switching to model '{}' with API key index {}...",
                        model, key_index
                    );
                }
                
                let mut fallback_request = request.clone();
                fallback_request.model = model.clone();
                
                let client_res = if key_index == 1 && model == &request.model {
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
                    Ok(response) => return Ok(response),
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
                            eprintln!("⏳ Rate limit or server overload detected. Pausing for 2 seconds before retry...");
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                        
                        last_error = Some(err);
                        key_index += 1;
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
        let _lock = Self::apply_api_pause().await;
        
        let mut models_to_try = vec![request.model.clone()];
        let stable_model = crate::providers::resolve_model_alias("stable");
        let gemini_lite_model = crate::providers::resolve_model_alias("gemini-lite");
        
        if !models_to_try.contains(&stable_model) {
            models_to_try.push(stable_model);
        }
        if !models_to_try.contains(&gemini_lite_model) {
            models_to_try.push(gemini_lite_model);
        }
        
        let mut last_error = None;
        
        for model in &models_to_try {
            let mut key_index = 1;
            while Self::has_key_for_index(model, key_index) {
                if key_index > 1 || model != &request.model {
                    eprintln!(
                        "\n⚠️ Switching to model '{}' with API key index {}...",
                        model, key_index
                    );
                }
                
                let mut fallback_request = request.clone();
                fallback_request.model = model.clone();
                
                let client_res = if key_index == 1 && model == &request.model {
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
                    Ok(stream) => return Ok(stream),
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
                            eprintln!("⏳ Rate limit or server overload detected. Pausing for 2 seconds before retry...");
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                        
                        last_error = Some(err);
                        key_index += 1;
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
}
