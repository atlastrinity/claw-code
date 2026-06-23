//
//  SettingsReducer.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation
import ComposableArchitecture

// MARK: - Reducer

@Reducer
struct SettingsReducer {
    @ObservableState
    struct State: Equatable {
        var connection: ConnectionSettings = .default
        var agentConfig: AgentConfiguration = .default
        var appearance: AppearanceSettings = .default
        var notifications: NotificationSettings = .default
        var dataPrivacy: DataPrivacySettings = .default
        var about: AboutInfo = .default

        static let empty = Self()
    }

    enum Action: Equatable {
        // Connection
        case connectionUpdate(ConnectionSettings)
        case testConnection

        // Agent
        case agentConfigUpdate(AgentConfiguration)
        case setModel(String)
        case setPermission(PermissionLevel)
        case setPreset(String?)

        // Appearance
        case appearanceUpdate(AppearanceSettings)

        // Notifications
        case notificationUpdate(NotificationSettings)

        // Data & Privacy
        case dataPrivacyUpdate(DataPrivacySettings)
        case clearChatHistory
        case clearSessions
        case exportData

        // About
        case aboutUpdate(AboutInfo)
    }

    var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case .connectionUpdate(let settings):
                state.connection = settings
                return .none

            case .testConnection:
                return .none

            case .agentConfigUpdate(let config):
                state.agentConfig = config
                return .none

            case .setModel(let model):
                state.agentConfig.model = model
                return .none

            case .setPermission(let permission):
                state.agentConfig.permission = permission
                return .none

            case .setPreset(let preset):
                state.agentConfig.preset = preset
                return .none

            case .appearanceUpdate(let settings):
                state.appearance = settings
                return .none

            case .notificationUpdate(let settings):
                state.notifications = settings
                return .none

            case .dataPrivacyUpdate(let settings):
                state.dataPrivacy = settings
                return .none

            case .clearChatHistory:
                return .none

            case .clearSessions:
                return .none

            case .exportData:
                return .none

            case .aboutUpdate(let info):
                state.about = info
                return .none
            }
        }
    }
}

// MARK: - Settings Models

struct ConnectionSettings: Equatable {
    var serverURL: String = "ws://localhost:8080/ws"
    var port: Int = 8080
    var websocketPath: String = "/ws"
    var useTLS: Bool = false
    var autoReconnect: Bool = true
    var maxReconnectAttempts: Int = 5
    var heartbeatInterval: TimeInterval = 15.0

    static let `default` = Self()
}

struct AgentConfiguration: Equatable {
    var model: String = "sonnet"
    var permission: PermissionLevel = .workspaceWrite
    var preset: String?
    var maxTurns: Int = 100
    var language: String = "en"
    var ragEnabled: Bool = false
    var ragURL: String?

    static let `default` = Self()
}

enum PermissionLevel: String, CaseIterable, Equatable {
    case readOnly = "Read-Only"
    case workspaceWrite = "Workspace Write"
    case dangerFullAccess = "Full Access"
}

struct AppearanceSettings: Equatable {
    var theme: Theme = .dark
    var fontSize: FontSize = .medium
    var codeFont: FontFamily = .system
    var hapticsEnabled: Bool = true
    var animationSpeed: AnimationSpeed = .normal

    static let `default` = Self()
}

enum Theme: String, CaseIterable, Equatable {
    case light = "Light"
    case dark = "Dark"
    case system = "System"
}

enum FontSize: String, CaseIterable, Equatable {
    case small = "Small"
    case medium = "Medium"
    case large = "Large"
    case xLarge = "Extra Large"
}

enum FontFamily: String, CaseIterable, Equatable {
    case system = "System"
    case mono = "Monospace"
}

enum AnimationSpeed: String, CaseIterable, Equatable {
    case reduced = "Reduced"
    case normal = "Normal"
    case fast = "Fast"
}

struct NotificationSettings: Equatable {
    var runCompletion: Bool = true
    var errorAlerts: Bool = true
    var connectionLost: Bool = true
    var soundEnabled: Bool = true

    static let `default` = Self()
}

struct DataPrivacySettings: Equatable {
    var keepChatHistory: Bool = true
    var keepSessions: Bool = true
    var exportData: Bool = false

    static let `default` = Self()
}

struct AboutInfo: Equatable {
    var version: String = "1.0.0"
    var build: String = "1"
    var clawRepoURL: String = "https://github.com/claw-code/claw-code"
    var licenses: [String] = []

    static let `default` = Self()
}
