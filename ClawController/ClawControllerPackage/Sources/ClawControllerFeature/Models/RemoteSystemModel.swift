//
//  RemoteSystemModel.swift
//  ClawControllerFeature
//
//  Model for remote system data and state
//

import Foundation
import SwiftData

/// Represents the connection state of the remote system
@Observable
public final class ConnectionState {
    public var isConnected: Bool = false
    public var connectionStatus: ConnectionStatus = .disconnected
    public var lastConnectedAt: Date?
    public var connectionError: String?

    public enum ConnectionStatus: String, CaseIterable {
        case disconnected = "Disconnected"
        case connecting = "Connecting..."
        case connected = "Connected"
        case error = "Error"
    }

    public init() {}

    public func updateStatus(_ status: ConnectionStatus, error: String? = nil) {
        connectionStatus = status
        connectionError = error

        if status == .connected {
            lastConnectedAt = Date()
        } else if status == .disconnected {
            connectionError = nil
        }
    }
}

/// Represents a command sent to the remote system
public struct RemoteCommand: Identifiable, Sendable {
    public let id: UUID
    public let command: String
    public let timestamp: Date
    public var status: CommandStatus

    public enum CommandStatus: String, Sendable {
        case pending = "Pending"
        case sent = "Sent"
        case completed = "Completed"
        case failed = "Failed"

        public var localized: String {
            switch self {
            case .pending: return "Pending"
            case .sent: return "Sent"
            case .completed: return "Completed"
            case .failed: return "Failed"
            }
        }
    }

    public init(command: String, status: CommandStatus = .pending) {
        self.id = UUID()
        self.command = command
        self.timestamp = Date()
        self.status = status
    }
}

/// Represents the system information from the remote system
public struct SystemInfo: Identifiable, Codable, Sendable {
    public var id: UUID
    public var name: String
    public var operatingSystem: String
    public var version: String
    public var uptime: String
    public var cpuUsage: Double
    public var memoryUsage: Double
    public var diskUsage: Double
    public var lastUpdated: Date

    public init(
        name: String = "Remote System",
        operatingSystem: String = "Unknown",
        version: String = "1.0.0",
        uptime: String = "0s",
        cpuUsage: Double = 0,
        memoryUsage: Double = 0,
        diskUsage: Double = 0
    ) {
        self.id = UUID()
        self.name = name
        self.operatingSystem = operatingSystem
        self.version = version
        self.uptime = uptime
        self.cpuUsage = cpuUsage
        self.memoryUsage = memoryUsage
        self.diskUsage = diskUsage
        self.lastUpdated = Date()
    }
}

/// Represents a log entry from the remote system
public struct SystemLog: Identifiable, Codable, Sendable {
    public let id: UUID
    public let level: LogLevel
    public let message: String
    public let timestamp: Date

    public enum LogLevel: String, Codable, Sendable {
        case debug = "DEBUG"
        case info = "INFO"
        case warning = "WARNING"
        case error = "ERROR"
    }

    public init(level: LogLevel, message: String) {
        self.id = UUID()
        self.level = level
        self.message = message
        self.timestamp = Date()
    }
}
