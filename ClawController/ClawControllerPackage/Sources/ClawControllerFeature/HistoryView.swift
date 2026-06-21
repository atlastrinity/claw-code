//
//  HistoryView.swift
//  ClawControllerFeature
//
//  View showing detailed command history
//

import SwiftUI

struct HistoryView: View {
    @State private var commandHistory: [CommandHistoryEntry] = []
    @State private var selectedEntry: CommandHistoryEntry?
    @State private var searchText: String = ""

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 12) {
                    // Search Bar
                    HStack(spacing: 12) {
                        Image(systemName: "magnifyingglass")
                            .foregroundStyle(.secondary)

                        TextField("Search commands...", text: $searchText)
                            .textFieldStyle(.plain)

                        if !searchText.isEmpty {
                            Button(action: { searchText = "" }) {
                                Image(systemName: "xmark.circle.fill")
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    .padding()
                    .background(Color(.secondarySystemBackground))
                    .cornerRadius(12)

                    // Filter Tabs
                    HStack(spacing: 8) {
                        FilterTab(title: "All", isActive: true)
                        FilterTab(title: "Success", isActive: false)
                        FilterTab(title: "Failed", isActive: false)
                    }
                    .padding(.horizontal)

                    // History List
                    if !filteredHistory.isEmpty {
                        ForEach(filteredHistory) { entry in
                            HistoryRow(entry: entry)
                                .onTapGesture {
                                    selectedEntry = entry
                                }
                        }
                    } else {
                        VStack(spacing: 16) {
                            Image(systemName: "clock.arrow.circlepath")
                                .font(.system(size: 50))
                                .foregroundStyle(.secondary.opacity(0.5))

                            Text("No commands yet")
                                .font(.headline)
                                .foregroundStyle(.secondary)

                            Text("Execute a command to see it here")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }

                    Spacer()
                }
                .padding(.vertical, 20)
            }
            .navigationTitle("Command History")
            .sheet(item: $selectedEntry) { entry in
                CommandDetailSheet(entry: entry)
            }
        }
    }

    private var filteredHistory: [CommandHistoryEntry] {
        if searchText.isEmpty {
            return commandHistory
        }
        return commandHistory.filter { entry in
            entry.command.localizedCaseInsensitiveContains(searchText)
        }
    }
}

// MARK: - History Row

struct HistoryRow: View {
    let entry: CommandHistoryEntry

    var body: some View {
        HStack(spacing: 12) {
            // Status Icon
            Image(systemName: statusIcon)
                .font(.system(size: 16))
                .foregroundStyle(statusColor)

            // Command
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.command)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                Text(entry.timestamp, style: .relative)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            // Status Badge
            Text(statusText)
                .font(.caption2)
                .fontWeight(.medium)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(statusColor.opacity(0.15))
                .foregroundStyle(statusColor)
                .cornerRadius(6)
        }
        .padding()
        .background(Color(.secondarySystemBackground))
        .cornerRadius(10)
        .shadow(color: .black.opacity(0.05), radius: 2, x: 0, y: 1)
    }

    private var statusIcon: String {
        switch entry.status {
        case .executing:
            return "arrow.triangle.2.circlepath"
        case .success:
            return "checkmark.circle.fill"
        case .failed:
            return "xmark.circle.fill"
        case .cancelled:
            return "stop.circle.fill"
        case .pending:
            return "clock"
        }
    }

    private var statusColor: Color {
        switch entry.status {
        case .executing:
            return .blue
        case .success:
            return .green
        case .failed:
            return .red
        case .cancelled:
            return .gray
        case .pending:
            return .orange
        }
    }

    private var statusText: String {
        entry.status.rawValue.capitalized
    }
}

// MARK: - Filter Tab

struct FilterTab: View {
    let title: String
    let isActive: Bool

    var body: some View {
        Button(action: {}) {
            Text(title)
                .font(.subheadline)
                .fontWeight(isActive ? .semibold : .regular)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .background(isActive ? Color.blue.opacity(0.15) : Color.clear)
                .foregroundStyle(isActive ? .blue : .secondary)
                .cornerRadius(20)
        }
    }
}

// MARK: - Command Detail Sheet

struct CommandDetailSheet: View {
    let entry: CommandHistoryEntry
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    // Command
                    DetailSection(title: "Command") {
                        Text(entry.command)
                            .font(.body)
                            .padding()
                            .background(Color(.secondarySystemBackground))
                            .cornerRadius(8)
                    }

                    // Status
                    DetailSection(title: "Status") {
                        StatusBadge(status: entry.status)
                    }

                    // Timestamp
                    DetailSection(title: "Timestamp") {
                        Text(entry.timestamp, style: .date)
                        Text(entry.timestamp, style: .time)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    // Response (if available)
                    if let result = entry.result {
                        DetailSection(title: "Result") {
                            Text(result.stdout)
                                .font(.body)
                                .padding()
                                .background(Color(.secondarySystemBackground))
                                .cornerRadius(8)
                        }
                    }
                }
                .padding()
            }
            .navigationTitle("Command Details")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
    }
}

// MARK: - Detail Section

struct DetailSection<Content: View>: View {
    let title: String
    let content: Content

    init(title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.headline)
                .foregroundStyle(.secondary)

            content
        }
    }
}

// MARK: - Status Badge

struct StatusBadge: View {
    let status: CommandStatus

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: statusIcon)
            Text(statusText)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(statusColor.opacity(0.15))
        .foregroundStyle(statusColor)
        .cornerRadius(20)
    }

    private var statusIcon: String {
        switch status {
        case .executing:
            return "arrow.triangle.2.circlepath"
        case .success:
            return "checkmark.circle.fill"
        case .failed:
            return "xmark.circle.fill"
        case .cancelled:
            return "stop.circle.fill"
        case .pending:
            return "clock"
        }
    }

    private var statusColor: Color {
        switch status {
        case .executing:
            return .blue
        case .success:
            return .green
        case .failed:
            return .red
        case .cancelled:
            return .gray
        case .pending:
            return .orange
        }
    }

    private var statusText: String {
        status.rawValue.capitalized
    }
}

// MARK: - Preview

#Preview {
    HistoryView()
}
