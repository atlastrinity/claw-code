export const DEFAULT_IDLE_CHECK_INTERVAL_MS = 30 * 1000;

export interface IdleTimeoutParseResult {
  timeoutMs: number;
  rawValue: string | null;
  invalid: boolean;
}

export function parseIdleTimeoutEnv(options: {
  env: NodeJS.ProcessEnv;
  envKey: string;
  defaultTimeoutMs: number;
}): IdleTimeoutParseResult {
  const rawValue = options.env[options.envKey]?.trim() ?? null;
  if (!rawValue) {
    return {
      timeoutMs: options.defaultTimeoutMs,
      rawValue: null,
      invalid: false,
    };
  }

  const parsed = Number(rawValue);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return {
      timeoutMs: options.defaultTimeoutMs,
      rawValue,
      invalid: true,
    };
  }

  return {
    timeoutMs: Math.floor(parsed),
    rawValue,
    invalid: false,
  };
}
