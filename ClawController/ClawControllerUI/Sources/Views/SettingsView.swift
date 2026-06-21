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
        Form {
            // Connection Settings
            Section(header: Text("Connection Settings")) {
                TextField("Host", text: $host)
                    .textFieldStyle(.roundedBorder)

                TextField("Port", text: $port)
                    .textFieldStyle(.roundedBorder)
                    .keyboardType(.numberPad)

                if state.connectionType == .ssh {
                    TextField("Username", text: $username)
                        .textFieldStyle(.roundedBorder)

                    SecureField("Password", text: $password)
                        .textFieldStyle(.roundedBorder)

                    TextField("SSH Key Path", text: $sshKeyPath)
                        .textFieldStyle(.roundedBorder)
                }
            }

            // Connection Actions
            Section(header: Text("Actions")) {
                Button(action: {
                    saveAndConnect()
                }) {
                    Label("Connect", systemImage: "antenna.radiowaves.left.and.right")
                        .frame(maxWidth: .infinity, alignment: .center)
                }
                .disabled(state.isConnected)
                .buttonStyle(.borderedProminent)

                if state.isConnected {
                    Button(action: {
                        disconnect()
                    }) {
                        Label("Disconnect", systemImage: "xmark.circle.fill")
                            .frame(maxWidth: .infinity, alignment: .center)
                    }
                    .buttonStyle(.bordered)
                }
            }

            // Connection Status
            Section(header: Text("Connection Status")) {
                statusRow("Type", value: state.connectionType.rawValue)
                statusRow("Host", value: state.host)
                statusRow("Port", value: state.port)
                statusRow("Status", value: state.isConnected ? "Connected" : "Disconnected")
                statusRow("Latency", value: state.latency)
            }

            // About
            Section(header: Text("About")) {
                HStack {
                    Text("Version")
                    Spacer()
                    Text("1.0.0")
                        .foregroundColor(.secondary)
                }
            }
        }
        .navigationTitle("Settings")
        .onAppear {
            loadSettings()
        }
        .alert("Error", isPresented: Binding(
            get: { state.errorMessage != nil },
            set: { _ in state.clearError() }
        ), messages: {
            if let error = state.errorMessage {
                Text(error)
            }
        })
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
