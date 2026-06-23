import type {
  Transport,
  TransportSendOptions,
} from '@modelcontextprotocol/sdk/shared/transport.js';
import {
  isJSONRPCErrorResponse,
  isJSONRPCRequest,
  isJSONRPCResultResponse,
  type JSONRPCMessage,
} from '@modelcontextprotocol/sdk/types.js';

export interface McpRequestLifecycleObserver {
  onRequestStarted?: () => void;
  onRequestCompleted?: () => void;
}

function requestIdKey(id: string | number): string {
  return String(id);
}

function completedRequestIdKey(message: JSONRPCMessage): string | null {
  if (isJSONRPCResultResponse(message)) {
    return requestIdKey(message.id);
  }

  if (isJSONRPCErrorResponse(message) && message.id !== undefined) {
    return requestIdKey(message.id);
  }

  return null;
}

export function instrumentMcpRequestLifecycle(
  transport: Transport,
  observer: McpRequestLifecycleObserver,
): void {
  const pendingRequestIds = new Set<string>();
  const originalStart = transport.start.bind(transport);
  const originalSend = transport.send.bind(transport);
  let onMessageWrapped = false;

  const wrapOnMessage = (): void => {
    if (onMessageWrapped || !transport.onmessage) {
      return;
    }

    onMessageWrapped = true;
    const downstreamOnMessage = transport.onmessage;
    transport.onmessage = (message, extra) => {
      let startedRequestId: string | null = null;

      if (isJSONRPCRequest(message)) {
        const requestId = requestIdKey(message.id);
        if (!pendingRequestIds.has(requestId)) {
          pendingRequestIds.add(requestId);
          startedRequestId = requestId;
          observer.onRequestStarted?.();
        }
      }

      try {
        downstreamOnMessage(message, extra);
      } catch (error) {
        if (startedRequestId !== null && pendingRequestIds.delete(startedRequestId)) {
          observer.onRequestCompleted?.();
        }
        throw error;
      }
    };
  };

  transport.start = async (): Promise<void> => {
    wrapOnMessage();
    await originalStart();
  };

  transport.send = async (
    message: JSONRPCMessage,
    options?: TransportSendOptions,
  ): Promise<void> => {
    const completedRequestId = completedRequestIdKey(message);
    const completesPendingRequest =
      completedRequestId !== null && pendingRequestIds.delete(completedRequestId);

    try {
      await originalSend(message, options);
    } finally {
      if (completesPendingRequest) {
        observer.onRequestCompleted?.();
      }
    }
  };
}
