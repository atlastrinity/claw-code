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
        HackerTheme.styledView {
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
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack {
            Image(systemName: "clock.arrow.circlepath")
                .font(.system(size: 20))
                .foregroundColor(HackerTheme.accentColor)
            Text("Command History")
                .font(.headline)
                .foregroundColor(HackerTheme.accentColor)
            Spacer()
            if !state.commandHistory.isEmpty {
                Button(action: {
                    clearHistory()
                }) {
                    Text("Clear")
                        .font(.caption)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .overlay(
                            RoundedRectangle(cornerRadius: 0)
                                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                        )
                        .foregroundColor(HackerTheme.accentColor)
                }
                .buttonStyle(.plain)
            }
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "tray")
                .font(.system(size: 40))
                .foregroundColor(HackerTheme.accentColor)
            Text("No commands yet")
                .font(.subheadline)
                .foregroundColor(HackerTheme.accentColor)
            Text("Send your first command to see it here")
                .font(.caption)
                .foregroundColor(HackerTheme.accentColor)
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
                .foregroundColor(HackerTheme.accentColor)
                .frame(width: 24, height: 24)
                .overlay(
                    RoundedRectangle(cornerRadius: 0)
                        .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                )

            // Command Content
            VStack(alignment: .leading, spacing: 4) {
                // Command
                Text(entry.command)
                    .font(.subheadline)
                    .foregroundColor(HackerTheme.textColor)
                    .lineLimit(2)

                // Status
                HStack(spacing: 6) {
                    if entry.isSuccess {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundColor(HackerTheme.accentColor)
                        Text("Success")
                            .font(.caption)
                            .foregroundColor(HackerTheme.accentColor)
                    } else {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(.red)
                        Text("Failed")
                            .font(.caption)
                            .foregroundColor(.red)
                    }

                    Text(entry.timestamp)
                        .font(.caption2)
                        .foregroundColor(HackerTheme.accentColor)
                }

                // Output
                if !entry.output.isEmpty {
                    ScrollView {
                        Text(entry.output)
                            .font(.caption)
                            .foregroundColor(HackerTheme.textColor)
                            .lineSpacing(2)
                    }
                    .frame(maxHeight: 150)
                    .padding(8)
                    .background(HackerTheme.backgroundColor)
                    .overlay(
                        RoundedRectangle(cornerRadius: 0)
                            .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                    )
                }
            }
        }
        .padding(8)
        .background(HackerTheme.backgroundColor)
        .overlay(
            RoundedRectangle(cornerRadius: 0)
                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
        )
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
