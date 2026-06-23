//
//  CommandResult.swift
//  ClawControllerFeature
//
//  Model representing the result of a command execution
//

import Foundation

/// Result of command execution
@Observable
public final class CommandResult {
    public var success: Bool
    public var message: String
    public var data: [String: Any]
    public var timestamp: Date

    public init(
        success: Bool,
        message: String = "",
        data: [String: Any] = [:],
        timestamp: Date = Date()
    ) {
        self.success = success
        self.message = message
        self.data = data
        self.timestamp = timestamp
    }

    /// Create a success result
    public static func success(message: String = "Command executed successfully", data: [String: Any] = [:]) -> CommandResult {
        CommandResult(success: true, message: message, data: data)
    }

    /// Create a failure result
    public static func failure(message: String = "Command failed", data: [String: Any] = [:]) -> CommandResult {
        CommandResult(success: false, message: message, data: data)
    }
}
