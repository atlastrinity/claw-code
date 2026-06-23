import Foundation

public enum TaskStatus: String, Codable, Sendable {
    case pending
    case in_progress
    case completed
    case failed
}

public struct TaskNode: Identifiable, Codable, Sendable {
    public let id: String
    public let parentId: String?
    public let content: String
    public var status: TaskStatus
    
    public init(id: String, parentId: String? = nil, content: String, status: TaskStatus = .pending) {
        self.id = id
        self.parentId = parentId
        self.content = content
        self.status = status
    }
}
