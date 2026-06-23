//
//  WebSocketManager.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation
import Combine
import OSLog

public actor WebSocketManager {
    private var webSocketTask: URLSessionWebSocketTask?
    private var urlSession: URLSession
    private var receiveTask: Task<Void, Never>?
    private var heartbeatTimer: Timer?
    private var logger: Logger

    private(set) public var connectionState: ConnectionState
    private(set) public var eventStream: AsyncStream<ServerEvent>?
    private var eventContinuation: AsyncStream<ServerEvent>.Continuation?

    public init(url: URL, logger: Logger = .init(subsystem: "com.clawcode.controller", category: "WebSocket")) {
        self.urlSession = URLSession(configuration: .default)
        self.connectionState = .disconnected(reason: .manual)
        self.logger = logger
    }

    public func connect() async throws {
        guard connectionState == .disconnected(reason: .manual) else {
            throw WebSocketError.alreadyConnected
        }

        connectionState = .connecting(attempt: 1)

        do {
            let request = URLRequest(url: url)
            webSocketTask = urlSession.webSocketTask(with: request)
            receiveTask = Task {
                await receiveMessages()
            }

            // Start heartbeat
            startHeartbeat()

            connectionState = .connected(since: Date())
            logger.info("WebSocket connected")

        } catch {
            connectionState = .error(.connectionFailed(error.localizedDescription))
            logger.error("Connection failed: \(error.localizedDescription)")
            throw error
        }
    }

    public func disconnect() {
        stopHeartbeat()
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = nil
        receiveTask?.cancel()
        receiveTask = nil
        connectionState = .disconnected(reason: .manual)
        logger.info("WebSocket disconnected")
    }

    public func send(_ command: ClientCommand) async throws {
        guard case .connected = connectionState else {
            throw WebSocketError.notConnected
        }

        let message: String
        switch command {
        case .prompt(let text):
            message = """
            {
                "type": "prompt",
                "text": "\(text)"
            }
            """
        case .cancel:
            message = """
            {
                "type": "cancel"
            }
            """
        case .ping:
            message = """
            {
                "type": "ping"
            }
            """
        case .config(let payload):
            message = """
            {
                "type": "config",
                "payload": \(payload)
            }
            """
        }

        let data = message.data(using: .utf8)!
        let frame = URLSessionWebSocketTask.Message.data(data)
        await webSocketTask?.send(frame)
    }

    private func receiveMessages() async {
        while !Task.isCancelled {
            do {
                let message = try await webSocketTask?.receive()

                switch message {
                case .string(let text):
                    await parseAndEmitEvents(from: text)
                case .data(let data):
                    await parseAndEmitEvents(from: String(data: data, encoding: .utf8) ?? "")
                @unknown default:
                    break
                }
            } catch {
                if !Task.isCancelled {
                    connectionState = .error(.connectionFailed(error.localizedDescription))
                    logger.error("Receive error: \(error.localizedDescription)")
                }
                break
            }
        }
    }

    private func parseAndEmitEvents(from text: String) async {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        for line in lines {
            guard !line.isEmpty else { continue }

            guard let data = line.data(using: .utf8),
                  let event = try? JSONDecoder().decode(ServerEvent.self, from: data) else {
                logger.warning("Failed to parse event: \(line)")
                continue
            }

            eventContinuation?.yield(event)
        }
    }

    private func startHeartbeat() {
        stopHeartbeat()
        heartbeatTimer = Timer.scheduledTimer(withTimeInterval: 15.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self = self, case .connected = self.connectionState else { return }
                do {
                    try await self.send(.ping)
                } catch {
                    logger.warning("Heartbeat failed: \(error)")
                }
            }
        }
    }

    private func stopHeartbeat() {
        heartbeatTimer?.invalidate()
        heartbeatTimer = nil
    }

    public func createEventStream() -> AsyncStream<ServerEvent> {
        eventStream = AsyncStream { continuation in
            self.eventContinuation = continuation
        }
        return eventStream!
    }
}

public enum WebSocketError: LocalizedError {
    case alreadyConnected
    case notConnected
    case connectionFailed(String)
    case authenticationFailed(String)
    case networkError(Error)

    public var errorDescription: String? {
        switch self {
        case .alreadyConnected:
            return "Already connected"
        case .notConnected:
            return "Not connected to server"
        case .connectionFailed(let message):
            return "Connection failed: \(message)"
        case .authenticationFailed(let message):
            return "Authentication failed: \(message)"
        case .networkError(let error):
            return "Network error: \(error.localizedDescription)"
        }
    }
}
