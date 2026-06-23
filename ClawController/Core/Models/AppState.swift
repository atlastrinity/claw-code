//
//  AppState.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation
import ComposableArchitecture

public enum ConnectionState: Equatable {
    case disconnected(reason: DisconnectReason)
    case connecting(attempt: Int)
    case authenticating
    case connected(since: Date)
    case reconnecting(attempt: Int, maxAttempts: Int)
    case error(ConnectionError)
}

public enum DisconnectReason: String, Equatable {
    case network = "Network changed"
    case server = "Server disconnected"
    case timeout = "Connection timeout"
    case authFailed = "Authentication failed"
    case manual = "User disconnected"
}

public enum ConnectionError: Equatable {
    case connectionFailed(String)
    case authenticationFailed(String)
    case networkError(Error)
}

public struct AppState: Equatable {
    public var connectionState: ConnectionState
    public var messages: [ChatMessage]
    public var tasks: [TaskNode]
    public var toolCalls: [ToolCall]
    public var logs: [LogEntry]
    public var currentSession: AgentSession?
    public var settings: UserSettings

    public init(
        connectionState: ConnectionState = .disconnected(reason: .manual),
        messages: [ChatMessage] = [],
        tasks: [TaskNode] = [],
        toolCalls: [ToolCall] = [],
        logs: [LogEntry] = [],
        currentSession: AgentSession? = nil,
        settings: UserSettings = UserSettings()
    ) {
        self.connectionState = connectionState
        self.messages = messages
        self.tasks = tasks
        self.toolCalls = toolCalls
        self.logs = logs
        self.currentSession = currentSession
        self.settings = settings
    }
}

public struct AgentSession: Equatable {
    public let id: UUID
    public let workspace: String
    public let model: String
    public let preset: String?
    public let permission: String
    public let startedAt: Date
    public var endedAt: Date?
    public var totalTurns: Int
    public var totalToolCalls: Int
    public var tokenUsage: TokenUsage

    public init(
        id: UUID = UUID(),
        workspace: String,
        model: String,
        preset: String? = nil,
        permission: String,
        startedAt: Date = Date(),
        endedAt: Date? = nil,
        totalTurns: Int = 0,
        totalToolCalls: Int = 0,
        tokenUsage: TokenUsage = TokenUsage(promptTokens: 0, completionTokens: 0, totalTokens: 0)
    ) {
        self.id = id
        self.workspace = workspace
        self.model = model
        self.preset = preset
        self.permission = permission
        self.startedAt = startedAt
        self.endedAt = endedAt
        self.totalTurns = totalTurns
        self.totalToolCalls = totalToolCalls
        self.tokenUsage = tokenUsage
    }
}

public struct UserSettings: Equatable {
    public var serverUrl: String
    public var port: Int
    public var wsPath: String
    public var autoReconnect: Bool
    public var maxReconnectAttempts: Int
    public var heartbeatInterval: TimeInterval
    public var model: String
    public var permission: String
    public var preset: String?
    public var ragEnabled: Bool

    public init(
        serverUrl: String = "localhost",
        port: Int = 8080,
        wsPath: String = "/ws",
        autoReconnect: Bool = true,
        maxReconnectAttempts: Int = 10,
        heartbeatInterval: TimeInterval = 15.0,
        model: String = "sonnet",
        permission: String = "workspace-write",
        preset: String? = nil,
        ragEnabled: Bool = false
    ) {
        self.serverUrl = serverUrl
        self.port = port
        self.wsPath = wsPath
        self.autoReconnect = autoReconnect
        self.maxReconnectAttempts = maxReconnectAttempts
        self.heartbeatInterval = heartbeatInterval
        self.model = model
        self.permission = permission
        self.preset = preset
        self.ragEnabled = ragEnabled
    }
}

public struct LogEntry: Equatable, Identifiable {
    public let id: UUID
    public let level: LogLevel
    public let message: String
    public let timestamp: Date
    public let source: LogSource

    public init(
        id: UUID = UUID(),
        level: LogLevel,
        message: String,
        timestamp: Date = Date(),
        source: LogSource = .general
    ) {
        self.id = id
        self.level = level
        self.message = message
        self.timestamp = timestamp
        self.source = source
    }
}

public enum LogLevel: String, Equatable {
    case error = "ERROR"
    case warn = "WARN"
    case info = "INFO"
    case debug = "DEBUG"
    case trace = "TRACE"
}

public enum LogSource: String, Equatable {
    case websocket = "WebSocket"
    case assistant = "Assistant"
    case tools = "Tools"
    case connection = "Connection"
    case general = "General"
}
