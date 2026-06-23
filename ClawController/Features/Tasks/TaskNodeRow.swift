//
//  TaskNodeRow.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI

struct TaskNodeRow: View {
    let task: TaskNode
    let isExpanded: Bool
    let onTap: () -> Void
    let onTaskTap: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Main Task Row
            taskRow

            // Child Tasks (if expanded)
            if isExpanded && !task.children.isEmpty {
                childrenContainer
            }
        }
        .padding(.vertical, 4)
    }

    private var taskRow: some View {
        Button(action: onTaskTap) {
            HStack(alignment: .top, spacing: 12) {
                // Expand/Collapse Icon
                Button(action: onTap) {
                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.caption)
                        .foregroundColor(.textSecondary)
                        .frame(width: 20)
                }
                .buttonStyle(PlainButtonStyle())

                // Status Icon
                statusIcon
                    .frame(width: 20)

                // Task Title
                VStack(alignment: .leading, spacing: 2) {
                    Text(task.title)
                        .font(.body)
                        .foregroundColor(task.isActive ? .textPrimary : .textSecondary)
                        .lineLimit(2)

                    // Task Metadata
                    if let duration = task.duration {
                        HStack(spacing: 12) {
                            if let completedAt = task.completedAt {
                                Text(durationString(from: completedAt, to: Date()))
                                    .font(.caption2)
                                    .foregroundColor(.textTertiary)
                            }

                            if let childCount = childCountBadge {
                                Text("\(childCount) tasks")
                                    .font(.caption2)
                                    .foregroundColor(.textTertiary)
                            }
                        }
                    }
                }

                Spacer()

                // Right side indicators
                if task.status == .inProgress {
                    ProgressView()
                        .progressViewStyle(CircularProgressViewStyle(tint: .secondary))
                        .scaleEffect(0.7)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: 12)
                    .fill(Color.surface)
                    .overlay(
                        RoundedRectangle(cornerRadius: 12)
                            .strokeBorder(
                                task.isActive ?
                                    Color.primary.opacity(0.3) :
                                    Color.clear,
                                lineWidth: 1
                            )
                    )
            )
            .shadow(color: Color.black.opacity(0.1), radius: 4, x: 0, y: 2)
        }
        .buttonStyle(ScaleButtonStyle())
    }

    private var statusIcon: some View {
        Group {
            switch task.status {
            case .pending:
                Image(systemName: "circle")
                    .foregroundColor(.textTertiary)
            case .inProgress:
                Image(systemName: "circle.dashed")
                    .foregroundColor(.orange)
                    .symbolEffect(.pulse, options: .repeating)
            case .completed:
                Image(systemName: "checkmark.circle.fill")
                    .foregroundColor(.secondary)
            case .failed:
                Image(systemName: "xmark.circle.fill")
                    .foregroundColor(.error)
            case .skipped:
                Image(systemName: "arrowtriangle.right.circle.fill")
                    .foregroundColor(.textTertiary)
            }
        }
        .font(.body)
    }

    private var childrenContainer: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(task.children) { child in
                TaskNodeRow(
                    task: child,
                    isExpanded: isExpanded,
                    onTap: { /* Handled by parent */ },
                    onTaskTap: onTaskTap
                )
            }
        }
        .padding(.leading, 32)
        .padding(.bottom, 8)
    }

    private var childCountBadge: Int? {
        task.children.isEmpty ? nil : task.children.count
    }

    private func durationString(from start: Date, to end: Date) -> String {
        let interval = end.timeIntervalSince(start)
        let minutes = Int(interval / 60)
        let seconds = Int(interval.truncatingRemainder(dividingBy: 60))

        if minutes > 0 {
            return "\(minutes)m \(seconds)s"
        } else {
            return "\(seconds)s"
        }
    }
}

// MARK: - Custom Button Style

struct ScaleButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
            .animation(.spring(response: 0.3, dampingFraction: 0.75), value: configuration.isPressed)
    }
}

// MARK: - Preview

#Preview("TaskNodeRow - Completed") {
    ScrollView {
        VStack(spacing: 12) {
            TaskNodeRow(
                task: TaskNode(
                    id: UUID(),
                    title: "Implement authentication",
                    status: .completed,
                    depth: 0,
                    children: []
                ),
                isExpanded: false,
                onTap: {},
                onTaskTap: {}
            )

            TaskNodeRow(
                task: TaskNode(
                    id: UUID(),
                    title: "Build user profile",
                    status: .inProgress,
                    depth: 0,
                    children: [
                        TaskNode(
                            id: UUID(),
                            title: "Create profile form",
                            status: .pending,
                            depth: 1,
                            children: []
                        ),
                        TaskNode(
                            id: UUID(),
                            title: "Implement avatar upload",
                            status: .pending,
                            depth: 1,
                            children: []
                        )
                    ]
                ),
                isExpanded: true,
                onTap: {},
                onTaskTap: {}
            )

            TaskNodeRow(
                task: TaskNode(
                    id: UUID(),
                    title: "Write unit tests",
                    status: .failed,
                    depth: 0,
                    children: []
                ),
                isExpanded: false,
                onTap: {},
                onTaskTap: {}
            )
        }
        .padding()
    }
}

#Preview("TaskNodeRow - Expanded") {
    ScrollView {
        VStack(spacing: 12) {
            TaskNodeRow(
                task: TaskNode(
                    id: UUID(),
                    title: "Implement authentication",
                    status: .completed,
                    depth: 0,
                    children: [
                        TaskNode(
                            id: UUID(),
                            title: "Create login screen",
                            status: .completed,
                            depth: 1,
                            children: []
                        ),
                        TaskNode(
                            id: UUID(),
                            title: "Implement token refresh",
                            status: .completed,
                            depth: 1,
                            children: []
                        )
                    ]
                ),
                isExpanded: true,
                onTap: {},
                onTaskTap: {}
            )
        }
        .padding()
    }
}
