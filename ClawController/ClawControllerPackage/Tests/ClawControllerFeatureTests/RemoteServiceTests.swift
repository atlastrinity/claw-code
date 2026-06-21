//
//  RemoteServiceTests.swift
//  ClawControllerFeatureTests
//
//  Tests for RemoteService
//

import Testing
@testable import ClawControllerFeature

@Suite("RemoteService Tests")
struct RemoteServiceTests {

    @Test("Test connection initialization")
    func testConnectionInitialization() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        #expect(service.systemInfo.status == .disconnected)
        #expect(!service.isConnected)
    }

    @Test("Test connect and disconnect")
    func testConnectAndDisconnect() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        // Test disconnect (should be safe to call)
        service.disconnect()
        #expect(service.systemInfo.status == .disconnected)

        // Test connect
        try await service.connect()
        #expect(service.systemInfo.status == .connected)
        #expect(service.isConnected)

        // Test disconnect again
        service.disconnect()
        #expect(service.systemInfo.status == .disconnected)
        #expect(!service.isConnected)
    }

    @Test("Test command execution")
    func testCommandExecution() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        try await service.connect()
        defer { service.disconnect() }

        let result = try await service.executeCommand("test command")

        #expect(result.success)
        #expect(result.message.contains("successfully"))
        #expect(!service.getCommandHistory().isEmpty)
    }

    @Test("Test command history")
    func testCommandHistory() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        try await service.connect()
        defer { service.disconnect() }

        let historyBefore = service.getCommandHistory()
        #expect(historyBefore.isEmpty)

        // Execute multiple commands
        try await service.executeCommand("command 1")
        try await service.executeCommand("command 2")
        try await service.executeCommand("command 3")

        let historyAfter = service.getCommandHistory()
        #expect(historyAfter.count == 3)

        // Clear history
        service.clearCommandHistory()
        let historyAfterClear = service.getCommandHistory()
        #expect(historyAfterClear.isEmpty)
    }

    @Test("Test system info")
    func testSystemInfo() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        let info = service.getSystemInfo()
        #expect(info.status == .disconnected)
        #expect(info.version == "1.0.0")

        try await service.connect()
        defer { service.disconnect() }

        let connectedInfo = service.getSystemInfo()
        #expect(connectedInfo.status == .connected)
        #expect(connectedInfo.version == "1.0.0")
    }

    @Test("Test settings update")
    func testSettingsUpdate() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        let originalSettings = service.getSettings()
        #expect(originalSettings.host == "localhost")

        let newSettings = RemoteSettings(
            host: "192.168.1.1",
            port: 9000
        )

        service.updateSettings(newSettings)
        let updatedSettings = service.getSettings()

        #expect(updatedSettings.host == "192.168.1.1")
        #expect(updatedSettings.port == 9000)
    }

    @Test("Test error on command without connection")
    func testErrorWithoutConnection() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        let result = try await service.executeCommand("test")
        #expect(!result.success)
        #expect(result.message.contains("Not connected"))
    }

    @Test("Test command status transitions")
    func testCommandStatusTransitions() async throws {
        let settings = RemoteSettings()
        let service = RemoteService(settings: settings)

        try await service.connect()
        defer { service.disconnect() }

        let history = service.getCommandHistory()
        #expect(history.isEmpty)

        // Execute command
        try await service.executeCommand("test")
        let firstEntry = service.getCommandHistory().first
        #expect(firstEntry?.status == .success)

        // Execute another command
        try await service.executeCommand("test2")
        let secondEntry = service.getCommandHistory().first
        #expect(secondEntry?.status == .success)
    }
}
