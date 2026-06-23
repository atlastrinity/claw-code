//
//  ServerEvent.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation

public enum ServerEvent: Codable, Equatable {
    case runStarted(RunStartPayload)
    case turnStarted(turn: Int)
    case textDelta(delta: String)
    case turnCompleted(AssistantTurn)
    case toolResult(ToolResultPayload)
    case runEnded(ok: Bool)
    case error(ErrorPayload)
}

public struct RunStartPayload: Codable, Equatable {
    public let schema: String
    public let formatVersion: String
    public let workspace: String
    public let model: String
    public let stream: Bool
    public let permission: String
    public let preset: String?
    public let session: String?
    public let ragEnabled: Bool
}

public struct AssistantTurn: Codable, Equatable {
    public let stopReason: String
    public let usage: TokenUsage
    public let text: String
    public let toolCalls: [ToolCallPayload]
}

public struct ToolCallPayload: Codable, Equatable {
    public let id: String
    public let name: String
    public let arguments: [String: AnyCodable]
}

public struct ToolResultPayload: Codable, Equatable {
    public let name: String
    public let toolUseId: String
    public let isError: Bool
    public let output: String
    public let truncated: Bool
    public let outputLenChars: Int
}

public struct ErrorPayload: Codable, Equatable {
    public let message: String
}

public struct TokenUsage: Codable, Equatable {
    public let promptTokens: Int
    public let completionTokens: Int
    public let totalTokens: Int
}

public enum ClientCommand: Codable, Equatable {
    case prompt(text: String)
    case cancel
    case ping
    case config(payload: [String: AnyCodable])
}
