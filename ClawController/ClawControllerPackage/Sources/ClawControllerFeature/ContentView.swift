//
//  ContentView.swift
//  ClawControllerFeature
//
//  Main view for remote control interface
//

import SwiftUI

public struct ContentView: View {
    @State private var selectedTab: Tab = .dashboard
    @State private var connectionStatus: ConnectionState.ConnectionStatus = .disconnected
    @State private var commandInput: String = ""

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            NavigationStack {
                DashboardView(connectionStatus: $connectionStatus)
                    .navigationTitle("Dashboard")
            }
            .tabItem {
                Label("Dashboard", systemImage: "chart.bar.fill")
            }
            .tag(Tab.dashboard)

            NavigationStack {
                CommandsView(commandInput: $commandInput)
                    .navigationTitle("Commands")
            }
            .tabItem {
                Label("Commands", systemImage: "command")
            }
            .tag(Tab.commands)

            HistoryView() // HistoryView already has NavigationStack inside
                .tabItem {
                    Label("History", systemImage: "clock.arrow.circlepath")
                }
                .tag(Tab.history)

            NavigationStack {
                SettingsView()
                    .navigationTitle("Settings")
            }
            .tabItem {
                Label("Settings", systemImage: "gear")
            }
            .tag(Tab.settings)
        }
        .task {
            await checkConnectionStatus()
        }
    }

    private func checkConnectionStatus() async {
        // Check connection status periodically
        while true {
            try? await Task.sleep(for: .seconds(2))
            // In real app, check actual connection status
            connectionStatus = .disconnected
        }
    }
}

private enum Tab {
    case dashboard
    case commands
    case history
    case settings
}

// MARK: - Connection Status Indicator

struct ConnectionStatusIndicator: View {
    @Binding var status: ConnectionState.ConnectionStatus

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(statusColor)
                .frame(width: 10, height: 10)
            Text(statusText)
                .font(.caption)
                .fontWeight(.medium)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(statusColor.opacity(0.2))
        .cornerRadius(20)
    }

    private var statusColor: Color {
        switch status {
        case .disconnected:
            return .red
        case .connecting:
            return .orange
        case .connected:
            return .green
        case .error:
            return .gray
        }
    }

    private var statusText: String {
        switch status {
        case .disconnected:
            return "Disconnected"
        case .connecting:
            return "Connecting..."
        case .connected:
            return "Connected"
        case .error:
            return "Error"
        }
    }
}

// MARK: - Preview

#Preview {
    ContentView()
}
