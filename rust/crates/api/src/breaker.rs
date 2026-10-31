//! Shared δ-fractal circuit breakers for API providers.
//!
//! One `FractalRateLimiter` per client instance, registered into a
//! process-wide registry so diagnostics (`claw fractal`) can show live
//! breaker state across every provider client in use.
//!
//! [`gate_request`] is the single entry point providers use before sending a
//! request chain — it decides cooling, shedding and the δ-scaled pause, so
//! each provider client stays a few lines instead of a duplicated policy.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use runtime::fractal::{FractalRateLimiter, BREAKER_DECAY_PERIOD, MAX_BREAKER_PAUSE};

use crate::error::ApiError;

/// A breaker shared by all clones of one client, tagged with the
/// `{provider}#{model}` identity it was created under so every log line it
/// produces can be attributed without the caller passing a label around.
#[derive(Debug)]
pub struct BreakerHandle {
    pub label: String,
    limiter: Mutex<FractalRateLimiter>,
}

/// A breaker shared by all clones of one client.
pub type SharedBreaker = Arc<BreakerHandle>;

pub fn lock(breaker: &SharedBreaker) -> MutexGuard<'_, FractalRateLimiter> {
    breaker
        .limiter
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn new_handle(label: String, max_retries: u32) -> SharedBreaker {
    Arc::new(BreakerHandle {
        label,
        limiter: Mutex::new(FractalRateLimiter::new(
            BREAKER_BASE_PAUSE_SECS,
            breaker_ceiling(max_retries),
        )),
    })
}

/// Built-in retry-chain count when `CLAW_API_MAX_RETRIES` is unset or
/// unparseable.
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// `CLAW_API_MAX_RETRIES` is clamped into this range: a huge value would make
/// the breaker ceiling (and the per-chain backoff ramp) absurd.
const MAX_RETRIES_CEILING: u32 = 9;

/// Default first-attempt backoff, shared by both provider clients.
pub const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_secs(1);

/// Default backoff ceiling, shared by both provider clients.
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Retry chains per request, shared by both provider clients so the retry
/// policy and the δ breaker ceiling stay defined in one place.
#[must_use]
pub fn configured_max_retries() -> u32 {
    let from_env = std::env::var("CLAW_API_MAX_RETRIES")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT_MAX_RETRIES);
    from_env.min(MAX_RETRIES_CEILING)
}

/// Consecutive exhausted chains before the breaker opens: one ceiling per
/// retry allowance, so a request budget of `n` retries sheds after `n + 2`
/// failed chains rather than after an unrelated fixed count.
#[must_use]
pub fn breaker_ceiling(max_retries: u32) -> usize {
    usize::try_from(max_retries).unwrap_or(0) + 2
}

/// Base inter-chain pause the δ^L escalation ramps from.
const BREAKER_BASE_PAUSE_SECS: f64 = 1.0;

/// Build the default breaker schedule for a retry policy: base 1 s pause,
/// escalating one level per consecutive exhausted retry chain, up to
/// [`breaker_ceiling`] chains before the chaos point (load shedding) is
/// reached.
///
/// The result is deliberately *not* registered: it belongs to one client only.
/// Clients that serve a known model should adopt a [`shared_breaker`] instead,
/// so escalation survives reconnects and key rotation.
#[must_use]
pub fn new_breaker(max_retries: u32) -> SharedBreaker {
    new_handle("unbound".to_string(), max_retries)
}

/// The one breaker for `{provider}#{model}`, creating it on first use.
///
/// `ProviderClient` rebuilds a client for every rotated API key index, so a
/// per-client breaker resets to level 0 exactly when a degraded provider most
/// needs escalation. Keying by provider and model makes the δ ramp accumulate
/// across those clients, and keeps the diagnostics registry bounded by the
/// number of models in use rather than by client construction count.
#[must_use]
pub fn shared_breaker(provider: &str, model: &str, max_retries: u32) -> SharedBreaker {
    let id = format!("{provider}#{model}");
    let mut registry = REGISTRY.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(existing) = registry.get(&id) {
        let breaker = existing.clone();
        drop(registry);
        lock(&breaker).set_max_level(breaker_ceiling(max_retries));
        return breaker;
    }
    let breaker = new_handle(id.clone(), max_retries);
    registry.insert(id, breaker.clone());
    breaker
}

/// The breaker an API client would start with under the current environment.
///
/// Diagnostics in a process that has not constructed a client yet (`claw
/// fractal` run standalone) still have to report the schedule the running
/// sessions actually use, not the type default.
#[must_use]
pub fn configured_breaker() -> FractalRateLimiter {
    FractalRateLimiter::new(
        BREAKER_BASE_PAUSE_SECS,
        breaker_ceiling(configured_max_retries()),
    )
}

/// Keep a live breaker's shed ceiling in step with a client's retry policy, so
/// `with_retry_policy` cannot leave the breaker escalating to a stale ceiling.
pub fn set_ceiling(breaker: &SharedBreaker, max_retries: u32) {
    lock(breaker).set_max_level(breaker_ceiling(max_retries));
}

static REGISTRY: LazyLock<Mutex<BTreeMap<String, SharedBreaker>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// `(id, level, consecutive_failures, trips)` for every registered breaker,
/// sorted by id for stable output.
#[must_use]
pub fn snapshot() -> Vec<(String, usize, usize, usize)> {
    let registry = REGISTRY.lock().unwrap_or_else(PoisonError::into_inner);
    registry
        .iter()
        .map(|(id, breaker)| {
            let limiter = lock(breaker);
            (
                id.clone(),
                limiter.current_level,
                limiter.consecutive_failures,
                limiter.breaker_trips,
            )
        })
        .collect()
}

/// Apply breaker policy before a request chain.
///
/// * decays the breaker back toward level 0 once the failing episode is over,
/// * sheds the request with [`ApiError::CircuitOpen`] at the chaos point,
/// * otherwise awaits the δ-scaled (`δ^L`, capped at [`MAX_BREAKER_PAUSE`])
///   pause the breaker demands. Never holds the lock across the await, and
///   uses an async timer so tokio workers are not blocked.
pub async fn gate_request(breaker: &SharedBreaker) -> Result<(), ApiError> {
    let label = breaker.label.as_str();
    let wait = {
        let mut limiter = lock(breaker);
        let now = Instant::now();
        limiter.apply_decay(now);
        if limiter.is_at_chaos_point() {
            limiter.record_breaker_trip();
            tracing::warn!(
                breaker = %label,
                level = limiter.current_level,
                ceiling = limiter.max_level,
                consecutive_failures = limiter.consecutive_failures,
                trips = limiter.breaker_trips,
                recovery_secs = BREAKER_DECAY_PERIOD.as_secs(),
                "δ breaker at chaos point: shedding this request chain"
            );
            return Err(ApiError::CircuitOpen {
                consecutive_failures: limiter.consecutive_failures,
                recovery_secs: BREAKER_DECAY_PERIOD.as_secs(),
            });
        }
        limiter.reserve_pressure_pause(now, MAX_BREAKER_PAUSE)
    };

    if wait > Duration::ZERO {
        tracing::debug!(
            breaker = %label,
            pause_ms = wait.as_millis() as u64,
            "δ breaker inter-chain pause"
        );
        tokio::time::sleep(wait).await;
    }
    Ok(())
}

/// One-line summary of an [`ApiError`] safe to put in a log: status when the
/// error carries one, plus a truncated body. The body is capped because
/// provider error payloads routinely embed the whole request.
fn error_summary(error: &ApiError) -> String {
    const MAX_BODY: usize = 300;
    let text = error.to_string();
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= MAX_BODY {
        flat
    } else {
        let cut: String = flat.chars().take(MAX_BODY).collect();
        format!("{cut}… (+{} chars)", flat.chars().count() - MAX_BODY)
    }
}

/// Record an exhausted retry chain and log the escalation it caused.
///
/// Returns the breaker level after the failure. The level *before and after*
/// are both logged: the δ ramp is the whole point of the breaker, and a
/// transition that is invisible in the log cannot be distinguished from a
/// breaker that never escalates.
pub fn note_failure(breaker: &SharedBreaker, error: &ApiError) -> usize {
    let mut limiter = lock(breaker);
    let before = limiter.current_level;
    limiter.on_failure();
    let (after, failures, ceiling) = (
        limiter.current_level,
        limiter.consecutive_failures,
        limiter.max_level,
    );
    drop(limiter);

    if after > before {
        tracing::warn!(
            breaker = %breaker.label,
            level_before = before,
            level = after,
            ceiling,
            consecutive_failures = failures,
            error = %error_summary(error),
            "retry chain failed: δ breaker escalated"
        );
    } else {
        tracing::warn!(
            breaker = %breaker.label,
            level = after,
            ceiling,
            consecutive_failures = failures,
            error = %error_summary(error),
            "retry chain failed"
        );
    }
    after
}

/// Record a successful chain, logging only when it actually de-escalated the
/// breaker — a healthy provider must not add a line per request.
pub fn note_success(breaker: &SharedBreaker) {
    let mut limiter = lock(breaker);
    let before = limiter.current_level;
    limiter.on_success();
    let after = limiter.current_level;
    drop(limiter);
    if before > 0 {
        tracing::info!(
            breaker = %breaker.label,
            level_before = before,
            level = after,
            "chain succeeded: δ breaker de-escalated"
        );
    }
}

/// Escalate for a failure that has no [`ApiError`] to carry — an accepted
/// request whose stream produced nothing. Silent stalls are the case a log
/// reader most needs explained, because every HTTP-level signal said "200 OK".
pub fn note_stall(breaker: &SharedBreaker, reason: &str) -> usize {
    let mut limiter = lock(breaker);
    let before = limiter.current_level;
    limiter.on_failure();
    let (after, failures, ceiling) = (
        limiter.current_level,
        limiter.consecutive_failures,
        limiter.max_level,
    );
    drop(limiter);
    tracing::warn!(
        breaker = %breaker.label,
        level_before = before,
        level = after,
        ceiling,
        consecutive_failures = failures,
        reason = %reason,
        "request accepted but produced no content: δ breaker escalated"
    );
    after
}

/// Per-attempt backoff: `initial * 2^(attempt-1)`, capped at `max`.
///
/// This is the intra-chain schedule; the δ breaker governs what happens
/// *between* exhausted chains, so the two never fight over the same delay.
pub fn exponential_backoff(
    attempt: u32,
    initial: Duration,
    max: Duration,
) -> Result<Duration, ApiError> {
    let Some(multiplier) = 1_u32.checked_shl(attempt.saturating_sub(1)) else {
        return Err(ApiError::BackoffOverflow {
            attempt,
            base_delay: initial,
        });
    };
    Ok(initial
        .checked_mul(multiplier)
        .map_or(max, |delay| delay.min(max)))
}

/// Backoff plus full-range additive jitter in `[0, base]`, so concurrent
/// sessions and clones do not re-collide on the same wall-clock instant.
pub fn jittered_backoff(
    attempt: u32,
    initial: Duration,
    max: Duration,
) -> Result<Duration, ApiError> {
    let base = exponential_backoff(attempt, initial, max)?;
    Ok(base + jitter_for_base(base))
}

/// Process-wide counter that guarantees distinct jitter samples even when
/// the system clock resolution is coarser than consecutive retry sleeps.
static JITTER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Entropy drawn from the nanosecond wall clock mixed with a monotonic
/// counter and run through a splitmix64 finalizer; adequate for retry jitter
/// (no cryptographic requirement).
fn jitter_for_base(base: Duration) -> Duration {
    let base_nanos = u64::try_from(base.as_nanos()).unwrap_or(u64::MAX);
    if base_nanos == 0 {
        return Duration::ZERO;
    }
    let raw_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
        });
    let tick = JITTER_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut mixed = raw_nanos
        .wrapping_add(tick)
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^= mixed >> 31;
    // Inclusive upper bound: jitter may equal `base`, matching "up to base".
    let jitter_nanos = mixed % base_nanos.saturating_add(1);
    Duration::from_nanos(jitter_nanos)
}

#[cfg(test)]
mod tests {
    use super::{error_summary, new_breaker, note_failure, note_success};
    use crate::error::{ApiError, ApiErrorInfo};
    use reqwest::StatusCode;

    fn api_error(status: u16, message: &str) -> ApiError {
        ApiError::Api(Box::new(ApiErrorInfo {
            status: StatusCode::from_u16(status).expect("valid status"),
            error_type: None,
            message: Some(message.to_string()),
            request_id: None,
            body: message.to_string(),
            retryable: false,
            suggested_action: None,
            retry_after: None,
        }))
    }

    #[test]
    fn failure_escalates_and_success_de_escalates_the_shared_breaker() {
        let breaker = new_breaker(1);
        assert_eq!(breaker.label, "unbound");
        let error = api_error(401, "token expired or incorrect");
        assert_eq!(note_failure(&breaker, &error), 1);
        assert_eq!(note_failure(&breaker, &error), 2);
        note_success(&breaker);
        assert_eq!(
            super::lock(&breaker).current_level,
            1,
            "a success must pull the ramp back down one level"
        );
    }

    #[test]
    fn shared_breaker_carries_its_registry_identity() {
        let breaker = super::shared_breaker("glm", "glm-4.7-flash", 2);
        assert_eq!(breaker.label, "glm#glm-4.7-flash");
        assert_eq!(super::lock(&breaker).max_level, super::breaker_ceiling(2));
    }

    #[test]
    fn error_summary_keeps_the_status_and_caps_the_body() {
        let short = error_summary(&api_error(401, "token expired or incorrect"));
        assert!(short.contains("401"), "summary was: {short}");
        assert!(
            short.contains("token expired or incorrect"),
            "summary was: {short}"
        );

        let long = error_summary(&api_error(400, &"x".repeat(5_000)));
        assert!(
            long.len() < 500,
            "summary should be truncated, was {} chars",
            long.len()
        );
        assert!(long.contains("chars)"), "truncation marker missing: {long}");
    }

    #[test]
    fn error_summary_strips_control_characters() {
        let summary = error_summary(&api_error(500, "line1\nline2\ttab"));
        assert!(
            !summary.contains('\n') && !summary.contains('\t'),
            "summary was: {summary:?}"
        );
    }
}
