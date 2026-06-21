//! Unified error type for the provider pipeline + middleware + RAG integration.
//!
//! Three error families coexisted with fragmented semantics:
//!   - [`api::ApiError`] — network / provider-level failures
//!   - [`runtime::RuntimeError`] — generic string wrapper
//!   - `String` — raw strings from the RAG service
//!
//! `PipelineError` folds them into one enum so every layer can enrich context
//! (provider name, retry count, RAG degradation flag) without losing type info.

use std::fmt::{Display, Formatter};

use api::ApiError;
use runtime::RuntimeError;

/// Classifies which sub-system originated the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineErrorStage {
    /// Upstream model provider (Anthropic, OpenAI, Grok, …)
    Provider,
    /// RAG service (`claw-rag-service`)
    Rag,
    /// Middleware pipeline (permissions, hooks, tracing)
    Middleware,
    /// Generic runtime (session, compaction, …)
    Runtime,
}

impl Display for PipelineErrorStage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider => write!(f, "provider"),
            Self::Rag => write!(f, "rag"),
            Self::Middleware => write!(f, "middleware"),
            Self::Runtime => write!(f, "runtime"),
        }
    }
}

/// Unified error returned throughout the provider pipeline.
#[derive(Debug)]
pub enum PipelineError {
    /// Upstream model-provider failure (wraps [`ApiError`]).
    Provider {
        source: Box<ApiError>,
        provider: String,
    },

    /// RAG service was unreachable or returned an error.
    /// RAG failures are *never* fatal — the pipeline degrades gracefully.
    Rag {
        message: String,
        /// When `true` the caller should log + continue instead of aborting.
        is_degraded: bool,
    },

    /// An error originating inside a middleware component.
    Middleware { message: String, stage: String },

    /// Passthrough for the existing [`RuntimeError`] type.
    Runtime(RuntimeError),
}

// ---------------------------------------------------------------------------
// Classification helpers
// ---------------------------------------------------------------------------

impl PipelineError {
    /// Which sub-system produced this error?
    #[must_use]
    pub fn error_stage(&self) -> PipelineErrorStage {
        match self {
            Self::Provider { .. } => PipelineErrorStage::Provider,
            Self::Rag { .. } => PipelineErrorStage::Rag,
            Self::Middleware { .. } => PipelineErrorStage::Middleware,
            Self::Runtime(_) => PipelineErrorStage::Runtime,
        }
    }

    /// Whether the operation can be retried (e.g. after a back-off).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Provider { source, .. } => source.is_retryable(),
            Self::Rag { .. } => false, // RAG errors degrade, not retry
            Self::Middleware { .. } | Self::Runtime(_) => false,
        }
    }

    /// Whether this error is fatal to the whole turn.
    /// RAG errors are explicitly non-fatal.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        !matches!(
            self,
            Self::Rag {
                is_degraded: true,
                ..
            }
        )
    }

    /// Convenience: should the caller silently degrade (log + continue)?
    #[must_use]
    pub fn should_degrade(&self) -> bool {
        matches!(
            self,
            Self::Rag {
                is_degraded: true,
                ..
            }
        )
    }

    /// Build a degraded RAG error (non-fatal, log-only).
    #[must_use]
    pub fn rag_degraded(message: impl Into<String>) -> Self {
        Self::Rag {
            message: message.into(),
            is_degraded: true,
        }
    }

    /// Build a middleware-stage error.
    #[must_use]
    pub fn middleware(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Middleware {
            message: message.into(),
            stage: stage.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Display + Error
// ---------------------------------------------------------------------------

impl Display for PipelineError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider { source, provider } => {
                write!(f, "provider `{provider}` error: {source}")
            }
            Self::Rag {
                message,
                is_degraded,
            } => {
                if *is_degraded {
                    write!(f, "rag service degraded: {message}")
                } else {
                    write!(f, "rag service error: {message}")
                }
            }
            Self::Middleware { message, stage } => {
                write!(f, "middleware `{stage}` error: {message}")
            }
            Self::Runtime(inner) => write!(f, "{inner}"),
        }
    }
}

impl std::error::Error for PipelineError {}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl From<ApiError> for PipelineError {
    fn from(source: ApiError) -> Self {
        Self::Provider {
            provider: String::from("unknown"),
            source: Box::new(source),
        }
    }
}

impl From<RuntimeError> for PipelineError {
    fn from(source: RuntimeError) -> Self {
        Self::Runtime(source)
    }
}

/// Convert **back** to `RuntimeError` at the public-API boundary.
/// This keeps `ConversationRuntime::run_turn` signature unchanged.
impl From<PipelineError> for RuntimeError {
    fn from(error: PipelineError) -> Self {
        RuntimeError::new(error.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rag_degraded_is_non_fatal() {
        let err = PipelineError::rag_degraded("connection refused");
        assert!(!err.is_fatal());
        assert!(err.should_degrade());
        assert!(!err.is_retryable());
        assert_eq!(err.error_stage(), PipelineErrorStage::Rag);
        assert!(err.to_string().contains("degraded"));
    }

    #[test]
    fn middleware_error_is_fatal() {
        let err = PipelineError::middleware("PermissionMiddleware", "user denied");
        assert!(err.is_fatal());
        assert!(!err.should_degrade());
        assert_eq!(err.error_stage(), PipelineErrorStage::Middleware);
        assert!(err.to_string().contains("PermissionMiddleware"));
    }

    #[test]
    fn runtime_conversion_roundtrips() {
        let rt_err = RuntimeError::new("something broke");
        let pipeline_err = PipelineError::from(rt_err);
        assert_eq!(pipeline_err.error_stage(), PipelineErrorStage::Runtime);

        let back: RuntimeError = pipeline_err.into();
        assert!(back.to_string().contains("something broke"));
    }

    #[test]
    fn provider_error_classification() {
        // Use an ApiError::Api variant which is easy to construct
        let api_err = ApiError::Api(Box::new(api::ApiErrorInfo {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            error_type: Some("rate_limit".into()),
            message: Some("too many requests".into()),
            request_id: Some("req_123".into()),
            body: String::new(),
            retryable: true,
            suggested_action: None,
            retry_after: None,
        }));
        let pipeline_err = PipelineError::Provider {
            source: Box::new(api_err),
            provider: "test-provider".into(),
        };
        assert_eq!(pipeline_err.error_stage(), PipelineErrorStage::Provider);
        // 429 with retryable=true delegates correctly
        assert!(pipeline_err.is_retryable());
        assert!(pipeline_err.is_fatal());
        assert!(pipeline_err.to_string().contains("test-provider"));
    }

    #[test]
    fn provider_error_non_retryable() {
        let api_err = ApiError::Api(Box::new(api::ApiErrorInfo {
            status: reqwest::StatusCode::BAD_REQUEST,
            error_type: Some("invalid_request".into()),
            message: Some("bad input".into()),
            request_id: None,
            body: String::new(),
            retryable: false,
            suggested_action: None,
            retry_after: None,
        }));
        let pipeline_err = PipelineError::Provider {
            source: Box::new(api_err),
            provider: "anthropic".into(),
        };
        assert!(!pipeline_err.is_retryable());
        assert!(pipeline_err.is_fatal());
    }
}
