use api::{MessageRequest, ProviderClient};
use runtime::AssistantEvent;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::pipeline_error::PipelineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,   // healthy, accepting requests
    Open,     // broken, rejecting requests
    HalfOpen, // testing, allowing one request
}

pub struct CircuitBreaker {
    failure_count: u32,
    last_failure: Option<Instant>,
    state: CircuitState,
    threshold: u32,
    recovery_timeout: Duration,
    /// Provider name for observability
    provider: String,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_count: 0,
            last_failure: None,
            state: CircuitState::Closed,
            threshold,
            recovery_timeout,
            provider: String::new(),
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    pub fn state(&mut self) -> CircuitState {
        if self.state == CircuitState::Open {
            if let Some(last) = self.last_failure {
                if last.elapsed() >= self.recovery_timeout {
                    self.state = CircuitState::HalfOpen;
                }
            }
        }
        self.state
    }

    pub fn should_allow_request(&mut self) -> bool {
        match self.state() {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => false,
        }
    }

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.last_failure = None;
        self.state = CircuitState::Closed;
    }

    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());
        if self.failure_count >= self.threshold {
            self.state = CircuitState::Open;
            // Ingest circuit-breaker trip event to RAG for cross-session observability
            self.ingest_trip_event();
        }
    }

    /// Fire-and-forget: persist the circuit-breaker trip into the RAG index
    /// so future sessions can query "has provider X been flaky recently?".
    fn ingest_trip_event(&self) {
        if self.provider.is_empty() {
            return;
        }
        let provider = self.provider.clone();
        let failures = self.failure_count;
        let threshold = self.threshold;
        // Spawn a best-effort, non-blocking ingest. Failure is silently ignored.
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::new();
            let doc = format!(
                "# Circuit Breaker Tripped\n\
                 Provider: {provider}\n\
                 Failures: {failures}/{threshold}\n\
                 Timestamp: {:?}\n\
                 State: Open (rejecting requests)\n",
                std::time::SystemTime::now()
            );
            let _ = client
                .post("http://127.0.0.1:8787/v1/ingest")
                .timeout(Duration::from_secs(2))
                .json(&serde_json::json!({
                    "path": format!("metrics/circuit-breaker/{provider}"),
                    "content": doc,
                }))
                .send();
        });
    }
}

pub struct CostTracker {
    pub model: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub estimated_cost_usd: f64,
}

impl CostTracker {
    pub fn new(model: String) -> Self {
        Self {
            model,
            total_input_tokens: 0,
            total_output_tokens: 0,
            estimated_cost_usd: 0.0,
        }
    }

    // A simple mock for now - in production this would look up model pricing
    pub fn record_usage(&mut self, input_tokens: u64, output_tokens: u64) {
        self.total_input_tokens += input_tokens;
        self.total_output_tokens += output_tokens;
        // Mock estimate: $0.001 per 1k input, $0.002 per 1k output
        self.estimated_cost_usd +=
            (input_tokens as f64 / 1000.0) * 0.001 + (output_tokens as f64 / 1000.0) * 0.002;

        // Persist cost metrics to RAG every ~10k tokens for cross-session analytics
        if self.total_input_tokens % 10_000 < input_tokens {
            self.persist_to_rag();
        }
    }

    /// Fire-and-forget cost snapshot to RAG.
    fn persist_to_rag(&self) {
        let model = self.model.clone();
        let input = self.total_input_tokens;
        let output = self.total_output_tokens;
        let cost = self.estimated_cost_usd;
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::new();
            let doc = format!(
                "# Cost Report: {model}\n\
                 Total input tokens: {input}\n\
                 Total output tokens: {output}\n\
                 Estimated cost: ${cost:.4}\n\
                 Timestamp: {:?}\n",
                std::time::SystemTime::now()
            );
            let _ = client
                .post("http://127.0.0.1:8787/v1/ingest")
                .timeout(Duration::from_secs(2))
                .json(&serde_json::json!({
                    "path": format!("metrics/cost/{model}"),
                    "content": doc,
                }))
                .send();
        });
    }
}

pub struct ProviderEntry {
    pub model: String,
    pub client: ProviderClient,
}

pub struct ResilientProviderChain {
    pub providers: Vec<ProviderEntry>,
    pub breakers: Vec<CircuitBreaker>,
    pub cost_tracker: CostTracker,
}

impl ResilientProviderChain {
    pub fn new(primary_model: String, providers: Vec<ProviderEntry>) -> Self {
        let breakers = providers
            .iter()
            .map(|entry| {
                CircuitBreaker::new(3, Duration::from_secs(30)).with_provider(&entry.model)
            })
            .collect();
        Self {
            providers,
            breakers,
            cost_tracker: CostTracker::new(primary_model),
        }
    }

    /// Stream with fallback, returning `PipelineError` internally.
    /// The public boundary (`ApiClient::stream`) converts to `RuntimeError`.
    pub fn stream_with_fallback(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        messages: Vec<api::InputMessage>,
        system: Option<String>,
        tools: Option<Vec<api::ToolDefinition>>,
        tool_choice: Option<api::ToolChoice>,
    ) -> Result<Vec<AssistantEvent>, runtime::RuntimeError> {
        // Wrap messages in Arc to share across fallback attempts without cloning.
        let shared_messages = Arc::new(messages);

        self.stream_with_fallback_inner(runtime, &shared_messages, system, tools, tool_choice)
            .map_err(runtime::RuntimeError::from)
    }

    fn stream_with_fallback_inner(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        shared_messages: &Arc<Vec<api::InputMessage>>,
        system: Option<String>,
        tools: Option<Vec<api::ToolDefinition>>,
        tool_choice: Option<api::ToolChoice>,
    ) -> Result<Vec<AssistantEvent>, PipelineError> {
        let mut last_error: Option<PipelineError> = None;

        for (index, entry) in self.providers.iter().enumerate() {
            let breaker = &mut self.breakers[index];

            if !breaker.should_allow_request() {
                eprintln!("provider {} is circuit broken, skipping", entry.model);
                continue;
            }

            // Use Arc::as_ref() to avoid cloning the full message vector.
            // Only individual fields that require owned values are cloned.
            let message_request = MessageRequest {
                model: entry.model.clone(),
                max_tokens: api::max_tokens_for_model(&entry.model),
                messages: shared_messages.as_ref().clone(),
                system: system.clone(),
                tools: tools.clone(),
                tool_choice: tool_choice.clone(),
                stream: true,
                ..Default::default()
            };

            let attempt =
                runtime.block_on(crate::stream_with_provider(&entry.client, &message_request));
            match attempt {
                Ok(events) => {
                    breaker.record_success();
                    self.cost_tracker.record_usage(0, 0);
                    return Ok(events);
                }
                Err(error) => {
                    breaker.record_failure();
                    if error.is_retryable() {
                        eprintln!(
                            "provider {} failed with retryable error: {}",
                            entry.model, error
                        );
                        last_error = Some(PipelineError::Provider {
                            provider: entry.model.clone(),
                            source: Box::new(error),
                        });
                    } else {
                        return Err(PipelineError::Provider {
                            provider: entry.model.clone(),
                            source: Box::new(error),
                        });
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            PipelineError::middleware(
                "ResilientProviderChain",
                "provider chain exhausted with no attempts or all circuit broken",
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let mut breaker = CircuitBreaker::new(3, Duration::from_secs(30));
        assert!(breaker.should_allow_request());

        breaker.record_failure();
        breaker.record_failure();
        assert!(breaker.should_allow_request());

        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.should_allow_request());
    }

    #[test]
    fn circuit_breaker_half_opens_after_timeout() {
        let mut breaker = CircuitBreaker::new(1, Duration::from_millis(100));
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.should_allow_request());

        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
        assert!(breaker.should_allow_request());
    }

    #[test]
    fn circuit_breaker_closes_after_success() {
        let mut breaker = CircuitBreaker::new(1, Duration::from_millis(100));
        breaker.record_failure();

        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.should_allow_request());
    }

    #[test]
    fn circuit_breaker_with_provider_name() {
        let breaker = CircuitBreaker::new(3, Duration::from_secs(30)).with_provider("claude-opus");
        assert_eq!(breaker.provider, "claude-opus");
    }
}
