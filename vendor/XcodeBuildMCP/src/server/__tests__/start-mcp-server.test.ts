import { afterEach, describe, expect, it, vi } from 'vitest';
import { closeServerWithTimeout } from '../mcp-shutdown.ts';

describe('fast-exit server close', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('returns skipped when server is not available', async () => {
    await expect(closeServerWithTimeout(undefined, 50)).resolves.toBe('skipped');
  });

  it('returns closed when server close resolves quickly', async () => {
    const close = vi.fn(async () => undefined);
    await expect(closeServerWithTimeout({ close }, 50)).resolves.toBe('closed');
    expect(close).toHaveBeenCalledTimes(1);
  });

  it('returns rejected when server close throws', async () => {
    const close = vi.fn(async () => {
      throw new Error('close failed');
    });

    await expect(closeServerWithTimeout({ close }, 50)).resolves.toBe('rejected');
    expect(close).toHaveBeenCalledTimes(1);
  });

  it('times out when server close never settles', async () => {
    vi.useFakeTimers();
    const close = vi.fn(() => new Promise<void>(() => undefined));

    const outcomePromise = closeServerWithTimeout({ close }, 50);
    await vi.advanceTimersByTimeAsync(50);

    await expect(outcomePromise).resolves.toBe('timed_out');
    expect(close).toHaveBeenCalledTimes(1);
  });
});
