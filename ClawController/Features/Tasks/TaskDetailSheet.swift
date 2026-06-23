//
//  TaskDetailSheet.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI

struct TaskDetailSheet: View {
    let taskId: UUID
    @Binding var isPresented: Bool
    let onDismiss: () -> Void

    @State private var task: TaskNode?
    @State private var isLoading = true

    var body: some View {
        NavigationView {
            ScrollView {
                if isLoading {
                    ProgressView()
                        .scaleEffect(1.5)
                        .padding()
                } else if let task = task {
                    VStack(spacing: 20) {
                        // Task Header
                        VStack(spacing: 12) {
                            // Status Badge
                            HStack {
                                Spacer()
                                statusBadge
                                Spacer()
                            }

                            // Title
                            Text(task.title)
                                .font(.title2)
                                .fontWeight(.bold)
                                .multilineTextAlignment(.center)
                                .foregroundColor(.textPrimary)

                            // Task Info
                            HStack(spacing: 16) {
                                if let createdAt = task.createdAt {
                                    infoRow(
                                        icon: "calendar",
                                        title: "Created",
                                        value: formatDate(createdAt)
                                    )
                                }
                                if let completedAt = task.completedAt {
                                    infoRow(
                                        icon: "checkmark.circle",
                                        title: "Completed",
                                        value: formatDate(completedAt)
                                    )
                                }
                                if let duration = task.duration {
                                    infoRow(
                                        icon: "clock",
                                        title: "Duration",
                                        value: durationString(duration)
                                    )
                                }
                            }
                            .padding(.top, 8)
                        }

                        // Status Detail
                        VStack(spacing: 12) {
                            HStack {
                                Text("Status")
                                    .font(.headline)
                                    .foregroundColor(.textSecondary)
                                Spacer()
                                Text(statusText)
                                    .font(.headline)
                                    .foregroundColor(statusColor)
                            }
                            .padding(.vertical, 8)
                            .padding(.horizontal, 12)
                            .background(Color.surfaceElevated)
                            .cornerRadius(10)

                            // Progress Indicator (if in progress)
                            if task.status == .inProgress {
                                ProgressView()
                                    .scaleEffect(1.5)
                                Text("Task in progress...")
                                    .font(.caption)
                                    .foregroundColor(.textSecondary)
                            }
                        }

                        // Children Tasks
                        if !task.children.isEmpty {
                            VStack(alignment: .leading, spacing: 12) {
                                Text("Subtasks (\(task.children.count))")
                                    .font(.headline)
                                    .foregroundColor(.textPrimary)

                                ForEach(task.children) { child in
                                    ChildTaskRow(task: child)
                                }
                            }
                        }

                        // Duration Breakdown (if completed)
                        if task.status == .completed, let completedAt = task.completedAt, let createdAt = task.createdAt {
                            VStack(alignment: .leading, spacing: 12) {
                                Text("Duration Breakdown")
                                    .font(.headline)
                                    .foregroundColor(.textPrimary)

                                VStack(spacing: 8) {
                                    durationBar(
                                        label: "Total Duration",
                                        value: task.duration ?? 0,
                                        color: .primary
                                    )
                                    if let childDuration = calculateChildTotalDuration(children: task.children) {
                                        durationBar(
                                            label: "Subtasks",
                                            value: childDuration,
                                            color: .secondary
                                        )
                                        let remaining = (task.duration ?? 0) - childDuration
                                        if remaining > 0 {
                                            durationBar(
                                                label: "Direct Work",
                                                value: remaining,
                                                color: .orange
                                            )
                                        }
                                    }
                                }
                            }
                        }

                        // Associated Tool Calls (placeholder)
                        VStack(alignment: .leading, spacing: 12) {
                            Text("Associated Tool Calls")
                                .font(.headline)
                                .foregroundColor(.textPrimary)

                            HStack {
                                Image(systemName: "info.circle")
                                    .foregroundColor(.textTertiary)
                                Text("Tool calls will appear here when available")
                                    .font(.caption)
                                    .foregroundColor(.textSecondary)
                            }
                            .padding()
                            .background(Color.surfaceElevated)
                            .cornerRadius(10)
                        }
                        .padding(.bottom)
                    }
                    .padding()
                }
            }
            .navigationTitle("Task Details")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        onDismiss()
                    }
                }
            }
        }
        .onAppear {
            loadTaskDetails()
        }
    }

    // MARK: - Computed Properties

    private var statusBadge: some View {
        HStack {
            Image(systemName: statusIconName)
                .font(.caption)
            Text(statusText)
                .font(.caption)
                .fontWeight(.semibold)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(
            RoundedRectangle(cornerRadius: 20)
                .fill(statusColor.opacity(0.15))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .stroke(statusColor.opacity(0.3), lineWidth: 1)
        )
    }

    private var statusText: String {
        switch task?.status {
        case .pending:
            return "Pending"
        case .inProgress:
            return "In Progress"
        case .completed:
            return "Completed"
        case .failed:
            return "Failed"
        case .skipped:
            return "Skipped"
        case .none:
            return "Unknown"
        }
    }

    private var statusColor: Color {
        switch task?.status {
        case .pending:
            return .textTertiary
        case .inProgress:
            return .orange
        case .completed:
            return .secondary
        case .failed:
            return .error
        case .skipped:
            return .textTertiary
        case .none:
            return .textSecondary
        }
    }

    private var statusIconName: String {
        switch task?.status {
        case .pending:
            return "circle"
        case .inProgress:
            return "circle.dashed"
        case .completed:
            return "checkmark.circle.fill"
        case .failed:
            return "xmark.circle.fill"
        case .skipped:
            return "arrowtriangle.right.circle.fill"
        case .none:
            return "questionmark.circle"
        }
    }

    // MARK: - Helper Views

    private func infoRow(icon: String, title: String, value: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .font(.caption)
                .foregroundColor(.textTertiary)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.caption2)
                    .foregroundColor(.textSecondary)
                Text(value)
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundColor(.textPrimary)
            }
        }
    }

    private func durationBar(label: String, value: TimeInterval, color: Color) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(label)
                    .font(.caption)
                    .foregroundColor(.textSecondary)
                Spacer()
                Text(durationString(value))
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundColor(color)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    // Background bar
                    RoundedRectangle(cornerRadius: 4)
                        .fill(Color.surfaceElevated)
                        .frame(height: 6)

                    // Progress bar
                    RoundedRectangle(cornerRadius: 4)
                        .fill(color)
                        .frame(width: geometry.size.width * CGFloat(value / 100), height: 6)
                }
            }
        }
    }

    private func calculateChildTotalDuration(children: [TaskNode]) -> TimeInterval? {
        var total: TimeInterval = 0
        for child in children {
            total += child.duration ?? 0
            if let childDuration = calculateChildTotalDuration(children: child.children) {
                total += childDuration
            }
        }
        return total > 0 ? total : nil
    }

    private func formatDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }

    private func durationString(_ duration: TimeInterval) -> String {
        let minutes = Int(duration / 60)
        let seconds = Int(duration.truncatingRemainder(dividingBy: 60))

        if minutes > 0 {
            return "\(minutes)m \(seconds)s"
        } else {
            return "\(seconds)s"
        }
    }

    // MARK: - Actions

    private func loadTaskDetails() {
        // In a real app, this would fetch the task from a store/service
        // For now, we'll use a placeholder task
        isLoading = false
    }
}

// MARK: - Child Task Row

struct ChildTaskRow: View {
    let task: TaskNode

    var body: some View {
        HStack(spacing: 12) {
            // Status Icon
            statusIcon
                .frame(width: 20)

            // Task Info
            VStack(alignment: .leading, spacing: 2) {
                Text(task.title)
                    .font(.body)
                    .foregroundColor(.textPrimary)
                    .lineLimit(1)

                if let duration = task.duration {
                    Text(durationString(duration))
                        .font(.caption2)
                        .foregroundColor(.textTertiary)
                }
            }

            Spacer()

            // Child Count Badge
            if !task.children.isEmpty {
                Text("\(task.children.count)")
                    .font(.caption2)
                    .foregroundColor(.textTertiary)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.surfaceElevated)
                    .cornerRadius(10)
            }
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 12)
        .background(Color.surface)
        .cornerRadius(10)
    }

    private var statusIcon: some View {
        Group {
            switch task.status {
            case .pending:
                Image(systemName: "circle")
                    .font(.caption)
                    .foregroundColor(.textTertiary)
            case .inProgress:
                Image(systemName: "circle.dashed")
                    .font(.caption)
                    .foregroundColor(.orange)
            case .completed:
                Image(systemName: "checkmark.circle.fill")
                    .font(.caption)
                    .foregroundColor(.secondary)
            case .failed:
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundColor(.error)
            case .skipped:
                Image(systemName: "arrowtriangle.right.circle.fill")
                    .font(.caption)
                    .foregroundColor(.textTertiary)
            }
        }
    }

    private func durationString(_ duration: TimeInterval) -> String {
        let minutes = Int(duration / 60)
        let seconds = Int(duration.truncatingRemainder(dividingBy: 60))

        if minutes > 0 {
            return "\(minutes)m \(seconds)s"
        } else {
            return "\(seconds)s"
        }
    }
}

// MARK: - Preview

#Preview {
    TaskDetailSheet(
        taskId: UUID(),
        isPresented: .constant(true),
        onDismiss: {}
    )
}
