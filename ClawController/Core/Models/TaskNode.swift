//
//  TaskNode.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation

public enum TaskStatus: String, Codable, Equatable {
    case pending
    case inProgress
    case completed
    case failed
    case skipped
}

public struct TaskNode: Identifiable, Codable, Equatable {
    public let id: UUID
    public let title: String
    public let status: TaskStatus
    public let depth: Int
    public var children: [TaskNode]
    public let createdAt: Date
    public var completedAt: Date?
    public var duration: TimeInterval?
    public var associatedToolCalls: [String] // tool_use_ids

    public init(
        id: UUID = UUID(),
        title: String,
        status: TaskStatus = .pending,
        depth: Int = 0,
        children: [TaskNode] = [],
        createdAt: Date = Date(),
        completedAt: Date? = nil,
        duration: TimeInterval? = nil,
        associatedToolCalls: [String] = []
    ) {
        self.id = id
        self.title = title
        self.status = status
        self.depth = depth
        self.children = children
        self.createdAt = createdAt
        self.completedAt = completedAt
        self.duration = duration
        self.associatedToolCalls = associatedToolCalls
    }

    public var isCompleted: Bool {
        status == .completed || status == .skipped
    }

    public var isFailed: Bool {
        status == .failed
    }

    public var isInProgress: Bool {
        status == .inProgress
    }

    public var childCount: Int {
        children.reduce(0) { $0 + 1 + $1.childCount }
    }
}
