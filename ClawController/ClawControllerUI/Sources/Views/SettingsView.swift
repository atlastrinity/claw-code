//
//  SettingsView.swift
//  ClawControllerUI
//
//  Settings view for connection configuration
//

import SwiftUI

struct SettingsView: View {
    @Bindable var state: RemoteControllerState
    @State private var host: String = ""
    @State private var port: String = ""
    @State private var username: String = ""
    @State private var password: String = ""
    @State private var sshKeyPath: String = ""

    init(state: RemoteControllerState) {
        self.state = state
    }

    var body: some View {
        HackerTheme.styledView {
            Form {
                // Connection Settings
                Section(header: Text("Connection Settings").foregroundColor(HackerTheme.accentColor)) {
                    TextField("Host", text: $host)
                        .textFieldStyle(.plain)
                        .padding(8)
                        .overlay(
                            RoundedRectangle(cornerRadius: 0)
                                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                        )

                    TextField("Port", text: $port)
                        .textFieldStyle(.plain)
                        .keyboardType(.numberPad)
                        .padding(8)
                        .overlay(
                            RoundedRectangle(cornerRadius: 0)
                                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                        )

                    if state.connectionType == .ssh {
                        TextField("Username", text: $username)
                            .textFieldStyle(.plain)
                            .padding(8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 0)
                                    .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                            )

                        SecureField("Password", text: $password)
                            .textFieldStyle(.plain)
                            .padding(8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 0)
                                    .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                            )

                        TextField("SSH Key Path", text: $sshKeyPath)
                            .textFieldStyle(.plain)
                            .padding(8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 0)
                                    .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                            )
                    }
                }
                .listRowBackground(HackerTheme.backgroundColor)

                // Connection Actions
                Section(header: Text("Actions").foregroundColor(HackerTheme.accentColor)) {
                    Button(action: {
                        saveAndConnect()
                    }) {
                        Label("Connect", systemImage: "antenna.radiowaves.left.and.right")
                            .frame(maxWidth: .infinity, alignment: .center)
                            .foregroundColor(HackerTheme.accentColor)
                    }
                    .disabled(state.isConnected)
                    .buttonStyle(.plain)
                    .padding(8)
                    .overlay(
                        RoundedRectangle(cornerRadius: 0)
                            .stroke(HackerTheme.accentColor, lineWidth: 1)
                    )

                    if state.isConnected {
                        Button(action: {
                            disconnect()
                        }) {
                            Label("Disconnect", systemImage: "xmark.circle.fill")
                                .frame(maxWidth: .infinity, alignment: .center)
                                .foregroundColor(.red)
                        }
                        .buttonStyle(.plain)
                        .padding(8)
                        .overlay(
                            RoundedRectangle(cornerRadius: 0)
                                .stroke(Color.red, lineWidth: 1)
                        )
                    }
                }
                .listRowBackground(HackerTheme.backgroundColor)

                // Connection Status
                Section(header: Text("Connection Status").foregroundColor(HackerTheme.accentColor)) {
                    statusRow("Type", value: state.connectionType.rawValue)
                    statusRow("Host", value: state.host)
                    statusRow("Port", value: state.port)
                    statusRow("Status", value: state.isConnected ? "Connected" : "Disconnected")
                    statusRow("Latency", value: state.latency)
                }
                .listRowBackground(HackerTheme.backgroundColor)

                // About
                Section(header: Text("About").foregroundColor(HackerTheme.accentColor)) {
                    HStack {
                        Text("Version")
                            .foregroundColor(HackerTheme.textColor)
                        Spacer()
                        Text("1.0.0")
                            .foregroundColor(HackerTheme.accentColor)
                    }
                }
                .listRowBackground(HackerTheme.backgroundColor)
            }
            .scrollContentBackground(.hidden)
            .background(HackerTheme.backgroundColor)
        }
    }

    // MARK: - Status Row

    private func statusRow(_ label: String, value: String) -> some View {
        HStack {
            Text(label)
            Spacer()
            Text(value)
                .foregroundColor(.secondary)
        }
    }

    // MARK: - Actions

    private func saveAndConnect() {
        let validHost = host.trimmingCharacters(in: .whitespacesAndNewlines)
        let validPort = port.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !validHost.isEmpty else {
            state.errorMessage = "Host is required"
            return
        }

        guard !validPort.isEmpty else {
            state.errorMessage = "Port is required"
            return
        }

        guard let portInt = Int(validPort), portInt > 0 && portInt < 65536 else {
            state.errorMessage = "Port must be a valid number (1-65535)"
            return
        }

        // Save settings
        state.host = validHost
        state.port = validPort
        state.username = username
        state.password = password
        state.sshKeyPath = sshKeyPath

        // Connect
        Task {
            await state.connect()
        }
    }

    private func disconnect() {
        Task {
            await state.disconnect()
        }
    }

    private func loadSettings() {
        host = state.host
        port = state.port
        username = state.username
        password = state.password
        sshKeyPath = state.sshKeyPath
    }
}

// MARK: - Preview

struct SettingsView_Previews: PreviewProvider {
    static var previews: some View {
        SettingsView(state: RemoteControllerState())
            .preferredColorScheme(.dark)
    }
}
