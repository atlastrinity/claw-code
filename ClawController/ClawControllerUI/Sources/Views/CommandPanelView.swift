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
        HackerTheme.styledView {
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
        }
    }

    // MARK: - Panel Header

    private var panelHeader: some View {
        HStack {
            Image(systemName: "command")
                .font(.system(size: 20))
                .foregroundColor(HackerTheme.accentColor)
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
                .buttonStyle(.plain)
            }
        }
    }

    // MARK: - Command Input Field

    private var commandInputField: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Enter command:")
                .font(.caption)
                .foregroundColor(HackerTheme.accentColor)

            HStack {
                Image(systemName: "terminal")
                    .foregroundColor(HackerTheme.accentColor)
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
                            .foregroundColor(HackerTheme.accentColor)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(8)
            .background(HackerTheme.backgroundColor)
            .overlay(
                RoundedRectangle(cornerRadius: 0)
                    .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
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
                .foregroundColor(HackerTheme.accentColor)

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
                                .overlay(
                                    RoundedRectangle(cornerRadius: 0)
                                        .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                                )
                        }
                        .buttonStyle(.plain)
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
            .background(state.isConnected ? HackerTheme.accentColor.opacity(0.2) : Color.gray.opacity(0.1))
            .overlay(
                RoundedRectangle(cornerRadius: 0)
                    .stroke(HackerTheme.accentColor, lineWidth: 1)
            )
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
