//
//  RemoteControllerState.swift
//  ClawControllerFeature
//
//  Observable state manager for the remote controller
//

import Foundation
import Observation

/// Main state manager for the remote controller
@Observable
public final class RemoteControllerState {
    // MARK: - Connection State

    public var connectionStatus: ConnectionStatus
    public var connectionConfig: ConnectionConfig
    public var systemInfo: SystemInfo

    // MARK: - Command State

    public var commandHistory: [CommandHistoryEntry]
    public var currentCommand: String?
    public var isExecutingCommand: Bool

    // MARK: - Error State

    public var errorMessage: String?
    public var showErrorAlert: Bool

    // MARK: - UI State

    public var isSettingsViewPresented: Bool
    public var isHistoryViewPresented: Bool

    public init(
        connectionStatus: ConnectionStatus = .disconnected,
        connectionConfig: ConnectionConfig = ConnectionConfig(),
        systemInfo: SystemInfo = SystemInfo(),
        commandHistory: [CommandHistoryEntry] = [],
        currentCommand: String? = nil,
        isExecutingCommand: Bool = false,
        errorMessage: String? = nil,
        showErrorAlert: Bool = false,
        isSettingsViewPresented: Bool = false,
        isHistoryViewPresented: Bool = false
    ) {
        self.connectionStatus = connectionStatus
        self.connectionConfig = connectionConfig
        self.systemInfo = systemInfo
        self.commandHistory = commandHistory
        self.currentCommand = currentCommand
        self.isExecutingCommand = isExecutingCommand
        self.errorMessage = errorMessage
        self.showErrorAlert = showErrorAlert
        self.isSettingsViewPresented = isSettingsViewPresented
        self.isHistoryViewPresented = isHistoryViewPresented
    }

    // MARK: - Connection Actions

    public func setConnecting() {
        connectionStatus = .connecting
        errorMessage = nil
    }

    public func setConnected() {
        connectionStatus = .connected
        errorMessage = nil
    }

    public func setDisconnected() {
        connectionStatus = .disconnected
        errorMessage = nil
    }

    public func setError(_ error: String) {
        connectionStatus = .error
        errorMessage = error
    }

    public func clearError() {
        errorMessage = nil
        showErrorAlert = false
    }

    // MARK: - Command Actions

    public func addCommandHistoryEntry(_ entry: CommandHistoryEntry) {
        commandHistory.insert(entry, at: 0)
        // Keep only last 50 commands
        if commandHistory.count > 50 {
            commandHistory = Array(commandHistory.prefix(50))
        }
    }

    public func setExecutingCommand(_ command: String) {
        currentCommand = command
        isExecutingCommand = true
    }

    public func setCommandComplete(success: Bool, message: String, executionTime: TimeInterval) {
        guard let command = currentCommand else { return }

        let result = CommandResult(
            success: success,
            message: message
        )

        let entry = CommandHistoryEntry(
            command: command,
            status: success ? .success : .failed,
            result: result
        )

        addCommandHistoryEntry(entry)
        currentCommand = nil
        isExecutingCommand = false
    }

    public func setCommandFailed(message: String) {
        guard let command = currentCommand else { return }

        let result = CommandResult(
            success: false,
            message: message
        )

        let entry = CommandHistoryEntry(
            command: command,
            status: .failed,
            result: result
        )

        addCommandHistoryEntry(entry)
        currentCommand = nil
        isExecutingCommand = false
    }

    // MARK: - UI Actions

    public func showSettings() {
        isSettingsViewPresented = true
    }

    public func hideSettings() {
        isSettingsViewPresented = false
    }

    public func showHistory() {
        isHistoryViewPresented = true
    }

    public func hideHistory() {
        isHistoryViewPresented = false
    }

    public func show(error: String) {
        errorMessage = error
        showErrorAlert = true
    }

    public func hideError() {
        errorMessage = nil
        showErrorAlert = false
    }

    // MARK: - Reset

    public func reset() {
        connectionStatus = .disconnected
        connectionConfig = ConnectionConfig()
        systemInfo = SystemInfo()
        commandHistory = []
        currentCommand = nil
        isExecutingCommand = false
        errorMessage = nil
        showErrorAlert = false
        isSettingsViewPresented = false
        isHistoryViewPresented = false
    }
}

// MARK: - Convenience Initializers

extension RemoteControllerState {
    public static var preview: RemoteControllerState {
        RemoteControllerState(
            connectionStatus: .connected,
            connectionConfig: ConnectionConfig(host: "192.168.1.100", port: 8080),
            systemInfo: SystemInfo(
                operatingSystem: "Linux",
                version: "1.0.0",
                uptime: "86400",
                cpuUsage: 45.5,
                memoryUsage: 62.3
            ),
            commandHistory: [
                CommandHistoryEntry(
                    command: "system info",
                    status: .success,
                    startTime: Date(),
                    result: CommandResult(
                        success: true,
                        message: "System info retrieved successfully"
                    )
                )
            ]
        )
    }
}
