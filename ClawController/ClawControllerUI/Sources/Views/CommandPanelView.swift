//
//  CommandPanelView.swift
//  ClawControllerUI
//
//  SwiftUI view for command panel and input
//

import SwiftUI

struct CommandPanelView: View {
    @Bindable var state: RemoteControllerState
    @State private var commandInput: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Panel Header
            panelHeader

            // Command Input
            commandInputField

            // Quick Actions
            quickActions

            // Send Button
            sendButton
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(color: Color.black.opacity(0.1), radius: 4, x: 0, y: 2)
    }

    // MARK: - Panel Header

    private var panelHeader: some View {
        HStack {
            Image(systemName: "command")
                .font(.system(size: 20))
                .foregroundColor(.blue)
            Text("Command Panel")
                .font(.headline)
            Spacer()
            if state.errorMessage != nil {
                Button(action: {
                    state.clearError()
                }) {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundColor(.red)
                }
                .buttonStyle(.borderless)
            }
        }
    }

    // MARK: - Command Input Field

    private var commandInputField: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Enter command:")
                .font(.caption)
                .foregroundColor(.secondary)

            HStack {
                Image(systemName: "terminal")
                    .foregroundColor(.secondary)
                TextField("Type command...", text: $commandInput)
                    .textFieldStyle(.plain)
                    .autocapitalization(.none)
                    .keyboardType(.asciiCapable)
                    .onSubmit {
                        sendCommand()
                    }

                if !commandInput.isEmpty {
                    Button(action: {
                        commandInput = ""
                    }) {
                        Image(systemName: "delete.left")
                            .font(.caption)
                    }
                    .buttonStyle(.borderless)
                }
            }
            .padding(8)
            .background(Color(.tertiarySystemBackground))
            .cornerRadius(8)
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(Color(.separator), lineWidth: 1)
            )

            if let error = state.errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundColor(.red)
                    .padding(.top, 4)
            }
        }
    }

    // MARK: - Quick Actions

    private var quickActions: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Quick Actions:")
                .font(.caption)
                .foregroundColor(.secondary)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(quickActionCommands, id: \.self) { command in
                        Button(action: {
                            commandInput = command
                        }) {
                            Text(command)
                                .font(.caption)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 6)
                                .background(Color(.systemBlue))
                                .foregroundColor(.white)
                                .cornerRadius(6)
                        }
                        .buttonStyle(.borderless)
                    }
                }
            }
        }
    }

    private var quickActionCommands: [String] {
        [
            "status",
            "ls",
            "pwd",
            "top",
            "df -h",
            "ps aux",
            "uptime"
        ]
    }

    // MARK: - Send Button

    private var sendButton: some View {
        Button(action: {
            sendCommand()
        }) {
            HStack {
                Image(systemName: "arrow.up.circle.fill")
                Text("Send Command")
                    .fontWeight(.semibold)
            }
            .frame(maxWidth: .infinity)
            .padding()
            .background(state.isConnected ? Color(.systemBlue) : Color(.systemGray3))
            .foregroundColor(.white)
            .cornerRadius(8)
            .disabled(!state.isConnected)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Actions

    private func sendCommand() {
        guard !commandInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return
        }

        Task {
            let result = await state.sendCommand(commandInput)
            switch result {
            case .success(let response):
                // Success - response is shown in command history
                commandInput = ""
            case .failure(let error):
                // Error is stored in state.errorMessage
                print("Command failed: \(error.localizedDescription)")
            }
        }
    }
}

// MARK: - Preview

struct CommandPanelView_Previews: PreviewProvider {
    static var previews: some View {
        CommandPanelView(state: RemoteControllerState())
            .padding()
            .previewLayout(.sizeThatFits)
    }
}
