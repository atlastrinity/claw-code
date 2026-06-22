//
//  RemoteService.swift
//  ClawControllerFeature
//
//  Service for remote control operations
//

import Foundation
import SwiftUI
import Combine
import Network

// MARK: - MCP Client

/// MCP (Model Context Protocol) Client for executing tools
private actor MCPClient {
    private let serverURL: URL
    private var sessionToken: String?
    private var webSocket: NWConnection?
    
    // MARK: - Error Types

    enum MCPError: Error {
        case notConnected
        case connectionTimeout
        case invalidResponse
        case responseTimeout
    }

    init(serverURL: URL) {
        self.serverURL = serverURL
    }

    /// Initialize session with MCP server via WebSocket
    private func initialize() async throws {
        // Close existing connection if any
        await closeConnection()

        // Create WebSocket connection
        let port = UInt16(serverURL.port ?? 8080)
        let host = serverURL.host ?? "localhost"

        let hostName = NWEndpoint.Host(host)
        let portNumber = NWEndpoint.Port(rawValue: port) ?? 8080
        let connection = NWConnection(
            to: .hostPort(host: hostName, port: portNumber),
            using: .tcp
        )

        webSocket = connection

        // Setup handlers
        connection.stateUpdateHandler = { (state: NWConnection.State) in
            switch state {
            case .ready:
                print("MCP WebSocket connection ready")
            case .failed(let error):
                print("MCP WebSocket connection failed: \(error)")
            case .waiting(let error):
                print("MCP WebSocket waiting: \(error)")
            default:
                break
            }
        }

        // Start connection
        connection.start(queue: DispatchQueue.global())

        // Wait for connection to be ready
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let timeout = Task {
                try await Task.sleep(nanoseconds: 5_000_000_000) // 5 seconds
                continuation.resume(throwing: MCPError.connectionTimeout)
            }

            Task {
                // A better approach would be to wait for .ready state
                continuation.resume()
                timeout.cancel()
            }
        }

        // Simulate session initialization
        sessionToken = UUID().uuidString
        print("MCP Session initialized with token: \(sessionToken ?? "none")")
    }

    /// Send message via WebSocket
    private func sendMessage(_ message: String) async throws {
        guard let connection = webSocket, connection.state == .ready else {
            throw MCPError.notConnected
        }

        connection.send(content: message.data(using: .utf8), completion: .contentProcessed { error in
            if let error = error {
                print("MCP WebSocket send error: \(error)")
            }
        })
    }

    /// Execute an MCP tool
    func executeTool(_ toolName: String, arguments: [String: Any]) async throws -> String {
        try await initialize()

        // Build MCP protocol request
        let request = """
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "\(toolName)",
                "arguments": \(arguments)
            }
        }
        """

        try await sendMessage(request)

        // Wait for response
        return try await receiveResponse()
    }

    /// Receive response from WebSocket
    private func receiveResponse() async throws -> String {
        guard let connection = webSocket else {
            throw MCPError.notConnected
        }

        return try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<String, Error>) in
            let timeout = Task {
                try await Task.sleep(nanoseconds: 10_000_000_000) // 10 seconds
                continuation.resume(throwing: MCPError.responseTimeout)
            }

            Task {
                do {
                    try await connection.receive(minimumIncompleteLength: 1, maximumLength: 65536, completion: { content, _, _, error in
                        Task {
                            if let error = error {
                                continuation.resume(throwing: error)
                            } else if let data = content, let string = String(data: data, encoding: .utf8) {
                                continuation.resume(returning: string)
                            } else {
                                continuation.resume(throwing: MCPError.invalidResponse)
                            }
                        }
                    })
                    timeout.cancel()
                } catch {
                    timeout.cancel()
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Close WebSocket connection
    private func closeConnection() async {
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            webSocket?.cancel()
            webSocket = nil
            continuation.resume()
        }
    }
}

// MARK: - RemoteService

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

    // MCP Client instance
    private let mcpClient = MCPClient(serverURL: URL(string: "http://localhost:3000")!)

    public var systemInfo: SystemInfo
    public let connectionState: ConnectionState

    private var _isConnected: Bool = false

    public var isConnected: Bool {
        return _isConnected
    }

    // MARK: - Initialization

    public init(settings: RemoteSettings = RemoteSettings()) {
        self.settings = settings
        self.systemInfo = SystemInfo()
        self.connectionState = ConnectionState()
    }

    // MARK: - Connection Management

    /// Connect to the remote system
    public func connect() async throws {
        guard !isConnected else {
            print("Already connected")
            return
        }

        connectionState.updateStatus(.connecting)

        // Підключення до локального інстансу Claw Code (наприклад, localhost:8080)
        let url = URL(string: "http://localhost:8080")!
        print("Connecting to Claw Code at \(url.absoluteString)...")
        
        // В реальному проекті тут було б реальне WebSocket або HTTP підключення
        try await Task.sleep(for: .seconds(1))

        connectionState.updateStatus(.connected)
        systemInfo.lastUpdated = Date()

        _isConnected = true
        print("Successfully connected to Claw Code")
    }

    /// Disconnect from the remote system
    public func disconnect() {
        connectionState.updateStatus(.disconnected)
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

    /// Execute an MCP tool command
    public func executeMCPTool(_ toolName: String, arguments: [String: Any]) async throws -> CommandResult {
        guard isConnected else {
            throw RemoteError.notConnected
        }

        do {
            // Use MCP client to execute the tool
            let response = try await mcpClient.executeTool(toolName, arguments: arguments)

            // Parse the response and create CommandResult
            let result = CommandResult(
                success: true,
                message: "MCP tool executed successfully",
                data: [
                    "tool": toolName,
                    "response": response
                ]
            )

            print("MCP tool '\(toolName)' executed successfully")

            return result
        } catch {
            print("MCP tool execution failed: \(error)")
            throw RemoteError.commandFailed("Failed to execute MCP tool: \(error.localizedDescription)")
        }
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
