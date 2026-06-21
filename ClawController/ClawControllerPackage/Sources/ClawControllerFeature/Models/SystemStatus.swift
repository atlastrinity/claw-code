//
//  SystemStatus.swift
//  ClawControllerFeature
//
//  Model representing system status and health information
//

import Foundation

/// Enum representing connection status
public enum ConnectionStatus: String, Codable, Sendable {
    case connecting = "connecting"
    case connected = "connected"
    case disconnected = "disconnected"
    case error = "error"
    case unknown = "unknown"

    public var localizedDescription: String {
        switch self {
        case .connecting:
            return "З'єднання..."
        case .connected:
            return "Підключено"
        case .disconnected:
            return "Відключено"
        case .error:
            return "Помилка з'єднання"
        case .unknown:
            return "Невідомо"
        }
    }

    public var colorName: String {
        switch self {
        case .connected:
            return "green"
        case .connecting:
            return "orange"
        case .disconnected, .error:
            return "red"
        case .unknown:
            return "gray"
        }
    }
}

/// Model representing system information
public struct SystemInfo: Codable, Sendable {
    public let version: String
    public let platform: String
    public let uptime: TimeInterval
    public let cpuUsage: Double
    public let memoryUsage: Double
    public let activeConnections: Int
    public let lastUpdated: Date

    public init(
        version: String = "1.0.0",
        platform: String = "Unknown",
        uptime: TimeInterval = 0,
        cpuUsage: Double = 0,
        memoryUsage: Double = 0,
        activeConnections: Int = 0,
        lastUpdated: Date = Date()
    ) {
        self.version = version
        self.platform = platform
        self.uptime = uptime
        self.cpuUsage = cpuUsage
        self.memoryUsage = memoryUsage
        self.activeConnections = activeConnections
        self.lastUpdated = lastUpdated
    }
}

/// Model representing command execution result
public struct CommandResult: Codable, Sendable {
    public let command: String
    public let success: Bool
    public let message: String
    public let executionTime: TimeInterval
    public let timestamp: Date

    public init(
        command: String,
        success: Bool,
        message: String,
        executionTime: TimeInterval,
        timestamp: Date = Date()
    ) {
        self.command = command
        self.success = success
        self.message = message
        self.executionTime = executionTime
        self.timestamp = timestamp
    }
}

/// Model representing command history entry
public struct CommandHistoryEntry: Identifiable, Codable, Sendable {
    public let id: UUID
    public let command: String
    public let status: CommandStatus
    public let timestamp: Date
    public let result: CommandResult?

    public init(
        id: UUID = UUID(),
        command: String,
        status: CommandStatus,
        timestamp: Date = Date(),
        result: CommandResult? = nil
    ) {
        self.id = id
        self.command = command
        self.status = status
        self.timestamp = timestamp
        self.result = result
    }
}

/// Enum representing command status
public enum CommandStatus: String, Codable, Sendable {
    case pending = "pending"
    case executing = "executing"
    case success = "success"
    case failed = "failed"

    public var localizedDescription: String {
        switch self {
        case .pending:
            return "Очікує"
        case .executing:
            return "Виконується"
        case .success:
            return "Успішно"
        case .failed:
            return "Помилка"
        }
    }
}

/// Model representing connection configuration
public struct ConnectionConfig: Codable, Sendable {
    public var host: String
    public var port: UInt16
    public var username: String?
    public var password: String?
    public var timeout: TimeInterval
    public var autoConnect: Bool

    public init(
        host: String = "localhost",
        port: UInt16 = 8080,
        username: String? = nil,
        password: String? = nil,
        timeout: TimeInterval = 30.0,
        autoConnect: Bool = false
    ) {
        self.host = host
        self.port = port
        self.username = username
        self.password = password
        self.timeout = timeout
        self.autoConnect = autoConnect
    }

    public var connectionString: String {
        if let username = username, let password = password {
            return "\(username):****@\(host):\(port)"
        } else {
            return "\(host):\(port)"
        }
    }
}
