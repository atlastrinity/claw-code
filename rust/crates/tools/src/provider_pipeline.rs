use std::time::{Duration, Instant};
use api::{ApiError, MessageRequest, ProviderClient};
use runtime::AssistantEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,     // healthy, accepting requests
    Open,       // broken, rejecting requests
    HalfOpen,   // testing, allowing one request
}

pub struct CircuitBreaker {
    failure_count: u32,
    last_failure: Option<Instant>,
    state: CircuitState,
    threshold: u32,
    recovery_timeout: Duration,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_count: 0,
            last_failure: None,
            state: CircuitState::Closed,
            threshold,
            recovery_timeout,
        }
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
        }
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
        self.estimated_cost_usd += (input_tokens as f64 / 1000.0) * 0.001 + (output_tokens as f64 / 1000.0) * 0.002;
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
        let breakers = providers.iter().map(|_| CircuitBreaker::new(3, Duration::from_secs(30))).collect();
        Self {
            providers,
            breakers,
            cost_tracker: CostTracker::new(primary_model),
        }
    }

    pub fn stream_with_fallback(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        messages: Vec<api::InputMessage>,
        system: Option<String>,
        tools: Option<Vec<api::ToolDefinition>>,
        tool_choice: Option<api::ToolChoice>,
    ) -> Result<Vec<AssistantEvent>, runtime::RuntimeError> {
        let mut last_error: Option<ApiError> = None;

        for (index, entry) in self.providers.iter().enumerate() {
            let breaker = &mut self.breakers[index];
            
            if !breaker.should_allow_request() {
                eprintln!("provider {} is circuit broken, skipping", entry.model);
                continue;
            }

            let message_request = MessageRequest {
                model: entry.model.clone(),
                max_tokens: api::max_tokens_for_model(&entry.model),
                messages: messages.clone(),
                system: system.clone(),
                tools: tools.clone(),
                tool_choice: tool_choice.clone(),
                stream: true,
                ..Default::default()
            };

            let attempt = runtime.block_on(crate::stream_with_provider(&entry.client, &message_request));
            match attempt {
                Ok(events) => {
                    breaker.record_success();
                    // Basic cost tracking logic (would be more advanced with actual token counts from API response)
                    // We just record a placeholder amount here or count tokens from events
                    self.cost_tracker.record_usage(0, 0); 
                    return Ok(events);
                }
                Err(error) => {
                    breaker.record_failure();
                    if error.is_retryable() {
                        eprintln!("provider {} failed with retryable error: {}", entry.model, error);
                        last_error = Some(error);
                    } else {
                        return Err(runtime::RuntimeError::new(error.to_string()));
                    }
                }
            }
        }

        Err(runtime::RuntimeError::new(last_error.map_or_else(
            || String::from("provider chain exhausted with no attempts or all circuit broken"),
            |error| error.to_string(),
        )))
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
}
