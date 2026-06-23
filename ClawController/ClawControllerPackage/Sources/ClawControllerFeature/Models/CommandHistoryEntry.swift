//
//  CommandHistoryEntry.swift
//  ClawControllerFeature
//
//  Model representing a command execution in history
//

import Foundation
import SwiftUI

/// Status of command execution
public enum CommandStatus: String, Sendable {
    case pending = "Pending"
    case executing = "Executing"
    case success = "Success"
    case failed = "Failed"
    case cancelled = "Cancelled"
}

/// Entry in command history
@Observable
public final class CommandHistoryEntry: Identifiable {
    public var id: UUID
    public var command: String
    public var status: CommandStatus
    public var startTime: Date
    public var endTime: Date?
    public var result: CommandResult?

    public init(
        command: String,
        status: CommandStatus = .pending,
        startTime: Date = Date(),
        endTime: Date? = nil,
        result: CommandResult? = nil
    ) {
        self.id = UUID()
        self.command = command
        self.status = status
        self.startTime = startTime
        self.endTime = endTime
        self.result = result
    }

    /// Mark command as executing
    public func markExecuting() {
        self.status = .executing
    }

    /// Mark command as completed with result
    public func markCompleted(result: CommandResult) {
        self.status = result.success ? .success : .failed
        self.endTime = Date()
        self.result = result
    }

    /// Mark command as cancelled
    public func markCancelled() {
        self.status = .cancelled
        self.endTime = Date()
    }

    /// Relative time string for display
    public var timestamp: String {
        if let endTime {
            let duration = endTime.timeIntervalSince(startTime)
            return duration < 1 ? "< 1s" : String(format: "%.1fs", duration)
        }
        return String(format: "%.1fs", Date().timeIntervalSince(startTime))
    }
}
