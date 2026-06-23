import { afterEach, describe, expect, it } from 'vitest';
import {
  areProcessStdioWritesSuppressed,
  isSentryCaptureSealed,
  resetShutdownStateForTests,
  sealSentryCapture,
  suppressProcessStdioWrites,
} from '../shutdown-state.ts';

afterEach(() => {
  resetShutdownStateForTests();
});

describe('shutdown-state', () => {
  it('suppresses stdio writes idempotently', () => {
    expect(areProcessStdioWritesSuppressed()).toBe(false);
    suppressProcessStdioWrites();
    suppressProcessStdioWrites();
    expect(areProcessStdioWritesSuppressed()).toBe(true);
  });

  it('seals sentry capture', () => {
    expect(isSentryCaptureSealed()).toBe(false);
    sealSentryCapture();
    expect(isSentryCaptureSealed()).toBe(true);
  });
});
