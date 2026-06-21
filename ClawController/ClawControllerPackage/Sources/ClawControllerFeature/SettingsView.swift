//
//  SettingsView.swift
//  ClawControllerFeature
//
//  Settings view for connection configuration
//

import SwiftUI

struct SettingsView: View {
    @State private var host: String = "localhost"
    @State private var port: Int = 8080
    @State private var timeout: Double = 30.0
    @State private var autoReconnect: Bool = true
    @State private var reconnectDelay: Double = 5.0
    @State private var showSuccessAlert: Bool = false

    var body: some View {
        Form {
            Section {
                HStack {
                    Text("Host")
                        .frame(width: 60, alignment: .leading)
                    TextField("localhost", text: $host)
                        .textFieldStyle(.roundedBorder)
                }

                HStack {
                    Text("Port")
                        .frame(width: 60, alignment: .leading)
                    Stepper("\(port)", value: $port, in: 1...65535)
                }

                HStack {
                    Text("Timeout (s)")
                        .frame(width: 100, alignment: .leading)
                    Stepper("\(Int(timeout))", value: $timeout, in: 5...300)
                }
            } header: {
                Text("Connection Settings")
            }

            Section {
                Toggle("Auto Reconnect", isOn: $autoReconnect)

                HStack {
                    Text("Reconnect Delay (s)")
                        .frame(width: 160, alignment: .leading)
                    Stepper("\(Int(reconnectDelay))", value: $reconnectDelay, in: 1...60)
                }
            } header: {
                Text("Reconnect Options")
            }

            Section {
                Button(action: saveSettings) {
                    Label("Save Settings", systemImage: "checkmark")
                }
            } footer: {
                Text("Changes will be applied when you reconnect")
            }
        }
        .navigationTitle("Settings")
        .alert("Settings Saved", isPresented: $showSuccessAlert) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Your connection settings have been saved successfully.")
        }
    }

    private func saveSettings() {
        // In real app, call RemoteService.updateSettings()
        print("Saving settings: \(host):\(port)")
        showSuccessAlert = true
    }
}

// MARK: - Preview

#Preview {
    SettingsView()
}
