//
//  ToolCall.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation

public enum ToolName: String, Codable, Equatable {
    case readFile
    case writeFile
    case listDir
    case grepWorkspace
    case globWorkspace
    case gitDiff
    case gitLog
    case retrieveContext
    case ingestContext
    case bash
    case execute
}

public struct ToolCall: Identifiable, Codable, Equatable {
    public let id: String
    public let name: ToolName
    public let arguments: [String: AnyCodable]
    public let result: ToolResult?
    public let startedAt: Date
    public var completedAt: Date?
    public var duration: TimeInterval?
    public var isError: Bool
    public var outputPreview: String?
    public var outputFullLength: Int?

    public init(
        id: String,
        name: ToolName,
        arguments: [String: AnyCodable],
        result: ToolResult? = nil,
        startedAt: Date = Date(),
        completedAt: Date? = nil,
        isError: Bool = false,
        outputPreview: String? = nil,
        outputFullLength: Int? = nil
    ) {
        self.id = id
        self.name = name
        self.arguments = arguments
        self.result = result
        self.startedAt = startedAt
        self.completedAt = completedAt
        self.isError = isError
        self.outputPreview = outputPreview
        self.outputFullLength = outputFullLength
        self.duration = completedAt.map { $0.timeIntervalSince(startedAt) }
    }
}

public struct ToolResult: Codable, Equatable {
    public let output: String
    public let isError: Bool
    public let truncated: Bool
    public let outputLenChars: Int

    public init(output: String, isError: Bool = false, truncated: Bool = false, outputLenChars: Int = 0) {
        self.output = output
        self.isError = isError
        self.truncated = truncated
        self.outputLenChars = outputLenChars
    }
}
