//
//  ToolCallCard.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI

struct ToolCallCard: View {
    let toolCall: ToolCall
    let isExpanded: Bool
    let onTap: () -> Void
    let onToolTap: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Main Card
            cardRow

            // Expanded Content
            if isExpanded {
                expandedContent
            }
        }
        .padding(.vertical, 4)
    }

    private var cardRow: some View {
        Button(action: onToolTap) {
            HStack(alignment: .top, spacing: 12) {
                // Tool Icon
                ZStack {
                    Circle()
                        .fill(toolIconColor.opacity(0.15))
                        .frame(width: 44, height: 44)

                    Image(systemName: toolIconName)
                        .font(.body)
                        .foregroundColor(toolIconColor)
                }

                // Tool Info
                VStack(alignment: .leading, spacing: 4) {
                    // Tool Name
                    Text(toolCall.name.rawValue)
                        .font(.body)
                        .fontWeight(.semibold)
                        .foregroundColor(.textPrimary)
                        .lineLimit(1)

                    // Metadata
                    HStack(spacing: 12) {
                        if let duration = toolCall.duration {
                            durationBadge(duration)
                        }

                        timeBadge

                        if !toolCall.outputPreview?.isEmpty ?? false {
                            previewBadge
                        }
                    }
                }

                Spacer()

                // Right side indicators
                HStack(spacing: 8) {
                    if toolCall.isError {
                        statusIcon(.error)
                    } else {
                        statusIcon(.success)
                    }

                    // Expand/Collapse Button
                    Button(action: onTap) {
                        Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                            .font(.caption)
                            .foregroundColor(.textSecondary)
                            .frame(width: 20)
                    }
                    .buttonStyle(PlainButtonStyle())
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
                                isError ? Color.error.opacity(0.3) : Color.primary.opacity(0.1),
                                lineWidth: 1
                            )
                    )
            )
            .shadow(color: Color.black.opacity(0.05), radius: 4, x: 0, y: 2)
        }
        .buttonStyle(ScaleButtonStyle())
    }

    private var expandedContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Duration Bar
            durationBar

            // Arguments
            if !toolCall.arguments.isEmpty {
                argumentsSection
            }

            // Output
            if let output = toolCall.result?.output, !output.isEmpty {
                outputSection
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Color.surfaceElevated)
        .cornerRadius(12)
        .padding(.leading, 32)
    }

    private var durationBar: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("Duration")
                    .font(.caption)
                    .foregroundColor(.textSecondary)

                Spacer()

                Text(durationString(toolCall.duration ?? 0))
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundColor(isError ? .error : .primary)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    // Background bar
                    RoundedRectangle(cornerRadius: 4)
                        .fill(Color.surfaceElevated)
                        .frame(height: 6)

                    // Progress bar
                    RoundedRectangle(cornerRadius: 4)
                        .fill(isError ? Color.error : (toolDurationColor))
                        .frame(width: geometry.size.width * CGFloat(toolDurationRatio), height: 6)
                }
            }
        }
    }

    private var argumentsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: "doc.text.fill")
                    .font(.caption)
                    .foregroundColor(.textTertiary)
                Text("Arguments")
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundColor(.textPrimary)
            }

            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(toolCall.arguments.keys.sorted()), id: \.self) { key in
                        HStack(spacing: 8) {
                            Text(key)
                                .font(.caption2)
                                .foregroundColor(.textSecondary)
                                .frame(width: 80, alignment: .leading)

                            Text(toolCall.arguments[key] ?? "")
                                .font(.caption)
                                .foregroundColor(.textPrimary)
                                .lineLimit(2)
                        }
                    }
                }
                .padding(.vertical, 4)
            }
            .frame(maxHeight: 100)
            .background(Color.surface)
            .cornerRadius(8)
        }
    }

    private var outputSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: isError ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .font(.caption)
                    .foregroundColor(.textTertiary)
                Text(isError ? "Error Output" : "Output")
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundColor(.textPrimary)
            }

            ScrollView {
                Text(toolCall.result?.output ?? "")
                    .font(.caption2)
                    .foregroundColor(.textPrimary)
                    .lineSpacing(2)
            }
            .frame(maxHeight: 150)
            .background(Color.surface)
            .cornerRadius(8)
        }
    }

    // MARK: - Supporting Views

    private var durationBadge: some View {
        HStack(spacing: 4) {
            Image(systemName: "clock")
                .font(.caption2)
            Text(durationString(toolCall.duration ?? 0))
                .font(.caption2)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(Color.surfaceElevated)
        .cornerRadius(8)
    }

    private var timeBadge: some View {
        HStack(spacing: 4) {
            Image(systemName: "clock.badge.checkmark")
                .font(.caption2)
            Text(timeString(from: toolCall.startedAt))
                .font(.caption2)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(Color.surfaceElevated)
        .cornerRadius(8)
    }

    private var previewBadge: some View {
        HStack(spacing: 4) {
            Image(systemName: "eye")
                .font(.caption2)
            Text("Preview")
                .font(.caption2)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(Color.surfaceElevated)
        .cornerRadius(8)
    }

    private var statusIcon: (Status) -> some View {
        let status: Status
        if toolCall.isError {
            status = .error
        } else {
            status = .success
        }

        return HStack(spacing: 6) {
            Image(systemName: statusIconName(for: status))
                .font(.caption)
            Text(statusText(for: status))
                .font(.caption)
                .fontWeight(.semibold)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(
            RoundedRectangle(cornerRadius: 20)
                .fill(statusColor(for: status).opacity(0.15))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .stroke(statusColor(for: status).opacity(0.3), lineWidth: 1)
        )
    }

    // MARK: - Computed Properties

    private var isError: Bool {
        toolCall.isError
    }

    private var toolIconName: String {
        switch toolCall.name {
        case .readFile:
            return "doc.text.fill"
        case .writeFile:
            return "doc.badge.plus.fill"
        case .listDir:
            return "folder.fill"
        case .grepWorkspace:
            return "magnifyingglass"
        case .globWorkspace:
            return "square.grid.2x2"
        case .gitDiff:
            return "square.and.arrow.right"
        case .gitLog:
            return "clock.arrow.circlepath"
        case .retrieveContext:
            return "brain"
        case .ingestContext:
            return "arrow.up.doc"
        }
    }

    private var toolIconColor: Color {
        switch toolCall.name {
        case .readFile:
            return .secondary
        case .writeFile:
            return .primary
        case .listDir:
            return .secondary
        case .grepWorkspace:
            return .orange
        case .globWorkspace:
            return .secondary
        case .gitDiff:
            return .primary
        case .gitLog:
            return .secondary
        case .retrieveContext:
            return .purple
        case .ingestContext:
            return .blue
        }
    }

    private var toolDurationColor: Color {
        let duration = toolCall.duration ?? 0
        if duration > 2.0 {
            return .orange
        } else if duration > 0.1 {
            return .secondary
        } else {
            return .green
        }
    }

    private var toolDurationRatio: Double {
        let duration = toolCall.duration ?? 0
        let maxDuration: TimeInterval = 3.0
        return min(duration / maxDuration, 1.0)
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

    private func timeString(from date: Date) -> String {
        let formatter = DateFormatter()
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }

    private func statusText(for status: Status) -> String {
        switch status {
        case .success:
            return "Success"
        case .error:
            return "Failed"
        }
    }

    private func statusIconName(for status: Status) -> String {
        switch status {
        case .success:
            return "checkmark.circle.fill"
        case .error:
            return "xmark.circle.fill"
        }
    }

    private func statusColor(for status: Status) -> Color {
        switch status {
        case .success:
            return .secondary
        case .error:
            return .error
        }
    }

    private enum Status {
        case success
        case error
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

#Preview("ToolCallCard - Success") {
    ScrollView {
        VStack(spacing: 12) {
            ToolCallCard(
                toolCall: ToolCall(
                    id: "1",
                    name: .readFile,
                    arguments: ["file": "/path/to/file.txt"],
                    startedAt: Date().addingTimeInterval(-3600),
                    duration: 150,
                    isError: false
                ),
                isExpanded: false,
                onTap: {},
                onToolTap: {}
            )

            ToolCallCard(
                toolCall: ToolCall(
                    id: "2",
                    name: .writeFile,
                    arguments: ["file": "/path/to/output.txt", "content": "test"],
                    startedAt: Date().addingTimeInterval(-3000),
                    duration: 50,
                    isError: false
                ),
                isExpanded: true,
                onTap: {},
                onToolTap: {}
            )

            ToolCallCard(
                toolCall: ToolCall(
                    id: "3",
                    name: .grepWorkspace,
                    arguments: ["pattern": "test"],
                    startedAt: Date().addingTimeInterval(-60),
                    duration: 2500,
                    isError: true,
                    outputPreview: "Error: pattern not found"
                ),
                isExpanded: false,
                onTap: {},
                onToolTap: {}
            )
        }
        .padding()
    }
}
