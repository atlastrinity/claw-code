//
//  CommandType.swift
//  ClawControllerFeature
//
//  Enum representing different command types
//

import Foundation

/// Command types supported by the remote controller
public enum CommandType: String, Codable, Sendable, CaseIterable {
    case systemInfo = "System Info"
    case restart = "Restart"
    case shutdown = "Shutdown"
    case update = "Update"
    case ping = "Ping"
    case backup = "Backup"
    case restore = "Restore"
    case logs = "View Logs"
    case diagnostics = "Run Diagnostics"

    public var description: String {
        return rawValue
    }
}
