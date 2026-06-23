import { afterEach, describe, expect, it } from 'vitest';
import { __mapLogLevelToSentryForTests, __shouldCaptureToSentryForTests } from '../logger.ts';
import { resetShutdownStateForTests, sealSentryCapture } from '../shutdown-state.ts';

describe('logger sentry capture policy', () => {
  afterEach(() => {
    resetShutdownStateForTests();
  });
  it('does not capture by default', () => {
    expect(__shouldCaptureToSentryForTests()).toBe(false);
  });

  it('does not capture when sentry is false', () => {
    expect(__shouldCaptureToSentryForTests({ sentry: false })).toBe(false);
  });

  it('captures only when explicitly enabled', () => {
    expect(__shouldCaptureToSentryForTests({ sentry: true })).toBe(true);
  });

  it('does not capture after sentry sealing', () => {
    sealSentryCapture();
    expect(__shouldCaptureToSentryForTests({ sentry: true })).toBe(false);
  });

  it('maps internal levels to Sentry log levels', () => {
    expect(__mapLogLevelToSentryForTests('emergency')).toBe('fatal');
    expect(__mapLogLevelToSentryForTests('warn')).toBe('warn');
    expect(__mapLogLevelToSentryForTests('notice')).toBe('info');
    expect(__mapLogLevelToSentryForTests('error')).toBe('error');
  });
});
