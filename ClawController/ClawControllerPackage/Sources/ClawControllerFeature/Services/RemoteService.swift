//
//  RemoteService.swift
//  ClawControllerFeature
//
//  Service layer for remote system operations
//

import Foundation
import Combine

/// Service for managing remote system connections and operations
@Observable
public final class RemoteService {
    // MARK: - Properties

    public let connectionState = ConnectionState()
    public var systemInfo = SystemInfo()
    public var commands: [RemoteCommand] = []
    public var logs: [SystemLog] = []

    private let updateInterval: TimeInterval = 5.0
    private var updateTimer: Timer?
    private var connectionTask: Task<Void, Never>?

    // MARK: - Initialization

    public init() {
        loadCommands()
        loadLogs()
    }

    deinit {
        stopMonitoring()
    }

    // MARK: - Connection Management

    /// Connect to remote system
    public func connect(host: String, port: Int) async throws {
        guard !connectionState.isConnected else {
            throw RemoteServiceError.alreadyConnected
        }

        connectionState.updateStatus(.connecting)

        // Simulate connection delay
        try await Task.sleep(for: .seconds(1))

        // Simulate connection success
        connectionState.isConnected = true
        connectionState.updateStatus(.connected)

        // Start monitoring system info
        startMonitoring()

        // Add initial log
        addLog(level: .info, message: "Connected to \(host):\(port)")
    }

    /// Disconnect from remote system
    public func disconnect() {
        stopMonitoring()
        connectionState.isConnected = false
        connectionState.updateStatus(.disconnected)

        addLog(level: .info, message: "Disconnected from remote system")
    }

    /// Check if currently connected
    public func isConnected() -> Bool {
        return connectionState.isConnected
    }

    // MARK: - Command Operations

    /// Send a command to remote system
    public func sendCommand(_ command: String) async -> Result<Void, RemoteServiceError> {
        guard connectionState.isConnected else {
            addLog(level: .error, message: "Cannot send command: not connected")
            return .failure(.notConnected)
        }

        let remoteCommand = RemoteCommand(command: command, status: .pending)
        commands.insert(remoteCommand, at: 0)

        // Simulate sending command
        try? await Task.sleep(for: .milliseconds(500))

        remoteCommand.status = .sent
        saveCommands()

        addLog(level: .info, message: "Command sent: \(command)")

        return .success(())
    }

    /// Get command history
    public func getCommands() -> [RemoteCommand] {
        return commands
    }

    /// Clear command history
    public func clearCommands() {
        commands.removeAll()
        saveCommands()
    }

    // MARK: - System Info Operations

    /// Refresh system information
    public func refreshSystemInfo() async throws {
        guard connectionState.isConnected else {
            throw RemoteServiceError.notConnected
        }

        // Simulate fetching system info
        try await Task.sleep(for: .milliseconds(300))

        // Update with simulated values
        systemInfo = SystemInfo(
            name: "Remote Server",
            operatingSystem: "Linux",
            version: "6.5.0",
            uptime: Date().timeIntervalSince1970 - 86400, // 1 day
            cpuUsage: Double.random(in: 10...80),
            memoryUsage: Double.random(in: 30...70),
            diskUsage: Double.random(in: 40...90)
        )

        addLog(level: .info, message: "System info refreshed")
    }

    /// Get current system info
    public func getSystemInfo() -> SystemInfo {
        return systemInfo
    }

    // MARK: - Log Operations

    /// Get log entries
    public func getLogs(limit: Int = 50) -> [SystemLog] {
        return Array(logs.prefix(limit))
    }

    /// Clear logs
    public func clearLogs() {
        logs.removeAll()
        saveLogs()
    }

    // MARK: - Private Methods

    private func startMonitoring() {
        updateTimer = Timer.scheduledTimer(withTimeInterval: updateInterval, repeats: true) { [weak self] _ in
            Task {
                try? await self?.refreshSystemInfo()
            }
        }
    }

    private func stopMonitoring() {
        updateTimer?.invalidate()
        updateTimer = nil
        connectionTask?.cancel()
    }

    private func addLog(level: SystemLog.LogLevel, message: String) {
        let log = SystemLog(level: level, message: message)
        logs.insert(log, at: 0)

        // Keep only last 100 logs
        if logs.count > 100 {
            logs = Array(logs.prefix(100))
        }

        saveLogs()
    }

    private func saveCommands() {
        // In real implementation, save to persistent storage
        // For now, we'll just keep them in memory
    }

    private func loadCommands() {
        // In real implementation, load from persistent storage
    }

    private func saveLogs() {
        // In real implementation, save to persistent storage
    }

    private func loadLogs() {
        // In real implementation, load from persistent storage
    }
}

// MARK: - Error Types

public enum RemoteServiceError: Error, LocalizedError {
    case notConnected
    case alreadyConnected
    case connectionFailed
    case commandFailed

    public var errorDescription: String? {
        switch self {
        case .notConnected:
            return "Not connected to remote system"
        case .alreadyConnected:
            return "Already connected to remote system"
        case .connectionFailed:
            return "Failed to connect to remote system"
        case .commandFailed:
            return "Failed to send command"
        }
    }
}
