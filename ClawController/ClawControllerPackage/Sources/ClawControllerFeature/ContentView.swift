//
//  ContentView.swift
//  ClawControllerFeature
//
//  Main view for remote control interface
//

import SwiftUI

public struct ContentView: View {
    @State private var selectedTab: Tab = .dashboard
    @State private var connectionStatus: ConnectionStatus = .disconnected
    @State private var commandInput: String = ""

    public init() {}

    public var body: some View {
        NavigationStack {
            TabView(selection: $selectedTab) {
                DashboardView(connectionStatus: $connectionStatus)
                    .tabItem {
                        Label("Dashboard", systemImage: "chart.bar.fill")
                    }
                    .tag(Tab.dashboard)

                CommandsView(commandInput: $commandInput)
                    .tabItem {
                        Label("Commands", systemImage: "command")
                    }
                    .tag(Tab.commands)

                HistoryView()
                    .tabItem {
                        Label("History", systemImage: "clock.arrow.circlepath")
                    }
                    .tag(Tab.history)

                SettingsView()
                    .tabItem {
                        Label("Settings", systemImage: "gear")
                    }
                    .tag(Tab.settings)
            }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Text("ClawController")
                        .font(.headline)
                }
                ToolbarItem(placement: .navigationBarTrailing) {
                    ConnectionStatusIndicator(status: $connectionStatus)
                }
            }
            .task {
                await checkConnectionStatus()
            }
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
    @Binding var status: ConnectionStatus

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
