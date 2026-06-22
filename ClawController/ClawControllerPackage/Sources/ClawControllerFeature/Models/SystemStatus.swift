////
//  SystemStatus.swift
//  ClawControllerFeature
////
//  Model representing system status and health information
////

import Foundation

/// Enum representing connection status
public enum ConnectionStatus: String, Codable, Sendable {
    case disconnected = "disconnected"
    case error = "error"
    case unknown = "unknown"
    case connecting = "connecting"
    case connected = "connected"

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