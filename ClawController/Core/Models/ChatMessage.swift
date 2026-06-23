//
//  ChatMessage.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation

public enum MessageRole: String, Codable, Equatable {
    case user
    case assistant
    case system
    case toolResult
}

public enum MessageContent: Codable, Equatable {
    case text(String)
    case markdown(String)
    case code(String, String)
    case image(Data)
    case toolCall(String, [String: AnyCodable])
    case error(String)
}

public enum MessageStatus: String, Codable, Equatable {
    case sending
    case sent
    case streaming
    case completed
    case failed
}

public struct ChatMessage: Identifiable, Codable, Equatable {
    public let id: UUID
    public let role: MessageRole
    public let content: MessageContent
    public let timestamp: Date
    public let metadata: MessageMetadata?
    public var status: MessageStatus

    public init(
        id: UUID = UUID(),
        role: MessageRole,
        content: MessageContent,
        timestamp: Date = Date(),
        metadata: MessageMetadata? = nil,
        status: MessageStatus = .sent
    ) {
        self.id = id
        self.role = role
        self.content = content
        self.timestamp = timestamp
        self.metadata = metadata
        self.status = status
    }
}

public struct MessageMetadata: Codable, Equatable {
    public let tokens: Int?
    public let latency: TimeInterval?
    public let model: String?
    public let sequenceNumber: Int?
}
