//
//  CommandHistoryView.swift
//  ClawControllerUI
//
//  SwiftUI view for command history
//

import SwiftUI

struct CommandHistoryView: View {
    @Bindable var state: RemoteControllerState

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            header

            // Command History List
            if state.commandHistory.isEmpty {
                emptyState
            } else {
                historyList
            }
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(color: Color.black.opacity(0.1), radius: 4, x: 0, y: 2)
    }

    // MARK: - Header

    private var header: some View {
        HStack {
            Image(systemName: "clock.arrow.circlepath")
                .font(.system(size: 20))
                .foregroundColor(.green)
            Text("Command History")
                .font(.headline)
            Spacer()
            if !state.commandHistory.isEmpty {
                Button(action: {
                    clearHistory()
                }) {
                    Text("Clear")
                        .font(.caption)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(Color(.systemRed))
                        .foregroundColor(.white)
                        .cornerRadius(4)
                }
                .buttonStyle(.borderless)
            }
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "tray")
                .font(.system(size: 40))
                .foregroundColor(.secondary)
            Text("No commands yet")
                .font(.subheadline)
                .foregroundColor(.secondary)
            Text("Send your first command to see it here")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 32)
    }

    // MARK: - History List

    private var historyList: some View {
        ScrollView {
            VStack(spacing: 8) {
                ForEach(Array(state.commandHistory.enumerated()), id: \.element.id) { index, entry in
                    commandRow(entry, index: index)
                }
            }
        }
        .frame(maxHeight: 400)
    }

    // MARK: - Command Row

    private func commandRow(_ entry: CommandHistoryEntry, index: Int) -> some View {
        HStack(alignment: .top, spacing: 12) {
            // Index Badge
            Text("\(index + 1)")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundColor(.secondary)
                .frame(width: 24, height: 24)
                .background(Color(.tertiarySystemBackground))
                .cornerRadius(4)

            // Command Content
            VStack(alignment: .leading, spacing: 4) {
                // Command
                Text(entry.command)
                    .font(.subheadline)
                    .foregroundColor(.primary)
                    .lineLimit(2)

                // Status
                HStack(spacing: 6) {
                    if entry.isSuccess {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundColor(.green)
                        Text("Success")
                            .font(.caption)
                            .foregroundColor(.green)
                    } else {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(.red)
                        Text("Failed")
                            .font(.caption)
                            .foregroundColor(.red)
                    }

                    Text(entry.timestamp)
                        .font(.caption2)
                        .foregroundColor(.secondary)
                }

                // Output
                if !entry.output.isEmpty {
                    ScrollView {
                        Text(entry.output)
                            .font(.caption)
                            .foregroundColor(.secondary)
                            .lineSpacing(2)
                    }
                    .frame(maxHeight: 150)
                    .padding(8)
                    .background(Color(.tertiarySystemBackground))
                    .cornerRadius(6)
                }
            }
        }
        .padding(8)
        .background(entry.isSuccess ? Color(.tertiarySystemBackground) : Color(.tertiarySystemBackground).opacity(0.8))
        .cornerRadius(8)
    }

    // MARK: - Actions

    private func clearHistory() {
        state.clearCommandHistory()
    }
}

// MARK: - Preview

struct CommandHistoryView_Previews: PreviewProvider {
    static var previews: some View {
        CommandHistoryView(state: RemoteControllerState())
            .padding()
            .previewLayout(.sizeThatFits)
    }
}
