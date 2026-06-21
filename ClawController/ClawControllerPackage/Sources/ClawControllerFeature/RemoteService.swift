//
//  RemoteService.swift
//  ClawControllerFeature
//
//  Service for remote control operations
//

import Foundation

/// Main service for remote control functionality
@available(iOS 17, *)
@Observable
public final class RemoteService {

    // MARK: - Properties

    @ObservationIgnored
    private var connectionTask: Task<Void, Never>?

    @ObservationIgnored
    private var commandQueue: [CommandHistoryEntry] = []

    private let settings: RemoteSettings

    public var systemInfo: SystemInfo

    private var _isConnected: Bool = false

    public var isConnected: Bool {
        return _isConnected
    }

    // MARK: - Initialization

    public init(settings: RemoteSettings = RemoteSettings()) {
        self.settings = settings
        self.systemInfo = SystemInfo(status: .disconnected)
    }

    // MARK: - Connection Management

    /// Connect to the remote system
    public func connect() async throws {
        guard !isConnected else {
            print("Already connected")
            return
        }

        systemInfo.status = .connecting

        // Simulate connection delay
        try await Task.sleep(for: .seconds(1))

        // Simulate connection success (replace with actual WebSocket connection)
        systemInfo.status = .connected
        systemInfo.lastUpdated = Date()

        // Simulate initial system info
        systemInfo = SystemInfo(
            status: .connected,
            version: "1.0.0",
            uptime: "2h 15m",
            cpuUsage: 45.2,
            memoryUsage: 62.8,
            lastUpdated: Date()
        )

        _isConnected = true
        print("Connected to remote system at \(settings.host):\(settings.port)")
    }

    /// Disconnect from the remote system
    public func disconnect() {
        systemInfo.status = .disconnected
        systemInfo.uptime = "0s"
        systemInfo.cpuUsage = 0.0
        systemInfo.memoryUsage = 0.0
        systemInfo.lastUpdated = Date()

        _isConnected = false
        commandQueue.removeAll()
        print("Disconnected from remote system")
    }

    /// Check connection status
    public func checkConnection() async throws -> Bool {
        if isConnected {
            // Simulate periodic status check
            try await Task.sleep(for: .milliseconds(100))

            // Update system info with simulated data
            systemInfo.cpuUsage = Double.random(in: 30...70)
            systemInfo.memoryUsage = Double.random(in: 50...80)
            systemInfo.lastUpdated = Date()

            return true
        } else {
            return false
        }
    }

    // MARK: - Command Execution

    /// Execute a command on the remote system
    public func executeCommand(_ command: String) async throws -> CommandResult {
        guard isConnected else {
            throw RemoteError.notConnected
        }

        // Add to command history
        let entry = CommandHistoryEntry(
            command: command,
            status: .executing
        )
        commandQueue.insert(entry, at: 0)

        // Simulate command execution
        try await Task.sleep(for: .milliseconds(500))

        // Simulate success response
        let result = CommandResult(
            success: true,
            message: "Command executed successfully",
            data: [
                "command": command,
                "status": "completed"
            ]
        )

        // Update history entry
        let historyEntry = CommandHistoryEntry(
            command: command,
            status: .success,
            startTime: Date(),
            endTime: Date(),
            result: result
        )
        commandQueue[0] = historyEntry

        print("Command '\(command)' executed successfully")
        return result
    }

    /// Send a quick command without waiting for full result
    public func sendCommand(_ command: String) async throws {
        try await executeCommand(command)
    }

    // MARK: - System Information

    /// Get current system information
    public func getSystemInfo() -> SystemInfo {
        systemInfo
    }

    /// Update system info with simulated data
    public func refreshSystemInfo() async throws {
        guard isConnected else {
            throw RemoteError.notConnected
        }

        try await Task.sleep(for: .milliseconds(200))

        systemInfo.cpuUsage = Double.random(in: 30...70)
        systemInfo.memoryUsage = Double.random(in: 50...80)
        systemInfo.lastUpdated = Date()
    }

    // MARK: - Command History

    /// Get command history
    public func getCommandHistory(limit: Int = 50) -> [CommandHistoryEntry] {
        Array(commandQueue.prefix(limit))
    }

    /// Clear command history
    public func clearCommandHistory() {
        commandQueue.removeAll()
        print("Command history cleared")
    }

    // MARK: - Settings

    /// Update connection settings
    public func updateSettings(_ newSettings: RemoteSettings) {
        self.settings.host = newSettings.host
        self.settings.port = newSettings.port
        self.settings.timeout = newSettings.timeout
        self.settings.autoConnect = newSettings.autoConnect
        self.settings.reconnectAttempts = newSettings.reconnectAttempts

        print("Settings updated: \(newSettings.host):\(newSettings.port)")
    }

    /// Get current settings
    public func getSettings() -> RemoteSettings {
        settings
    }

    // MARK: - Error Types

    public enum RemoteError: Error, LocalizedError {
        case notConnected
        case connectionFailed
        case commandFailed(String)
        case invalidSettings

        public var errorDescription: String? {
            switch self {
            case .notConnected:
                return "Not connected to remote system"
            case .connectionFailed:
                return "Failed to connect to remote system"
            case .commandFailed(let message):
                return "Command failed: \(message)"
            case .invalidSettings:
                return "Invalid connection settings"
            }
        }
    }
}
