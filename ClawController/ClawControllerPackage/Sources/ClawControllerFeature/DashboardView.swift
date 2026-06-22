//
//  DashboardView.swift
//  ClawControllerFeature
//
//  Dashboard view showing system information and quick actions
//

import SwiftUI

struct DashboardView: View {
    @Binding var connectionStatus: ConnectionState.ConnectionStatus
    @State private var systemInfo: SystemInfo = SystemInfo()
    @State private var isLoading: Bool = true
    @State private var showConnectionAlert: Bool = false

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                // Connection Card
                ConnectionCard(
                    status: connectionStatus,
                    isConnected: connectionStatus == .connected
                )
                .padding(.horizontal)

                // System Stats
                if connectionStatus == .connected {
                    SystemStatsView(systemInfo: systemInfo)
                        .padding(.horizontal)
                }

                // Quick Actions
                QuickActionsView()
                    .padding(.horizontal)

                Spacer()
            }
            .padding(.vertical, 20)
        }
        .refreshable {
            await refreshSystemInfo()
        }
        .overlay {
            if isLoading && connectionStatus == .disconnected {
                ProgressView("Connecting...")
            }
        }
        .task {
            await checkConnection()
        }
    }

    private func checkConnection() async {
        isLoading = true
        // Simulate connection check
        try? await Task.sleep(for: .seconds(1))
        connectionStatus = .disconnected
        isLoading = false
    }

    private func refreshSystemInfo() async {
        // Simulate refresh
        try? await Task.sleep(for: .seconds(0.5))
        // In real app, call RemoteService.refreshSystemInfo()
    }
}

// MARK: - Connection Card

struct ConnectionCard: View {
    let status: ConnectionState.ConnectionStatus
    let isConnected: Bool

    var body: some View {
        VStack(spacing: 16) {
            HStack {
                Image(systemName: statusIcon)
                    .font(.system(size: 40))
                    .foregroundStyle(statusColor)
                VStack(alignment: .leading, spacing: 4) {
                    Text(statusTitle)
                        .font(.title2)
                        .fontWeight(.bold)
                    Text(statusSubtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Spacer()
            }

            if isConnected {
                Button(action: {}) {
                    Label("Disconnect", systemImage: "power")
                        .frame(maxWidth: .infinity)
                        .padding()
                        .background(Color.red.opacity(0.1))
                        .foregroundStyle(.red)
                        .cornerRadius(12)
                }
            }
        }
        .padding()
        .frame(maxWidth: .infinity)
        .background(Color(.systemBackground))
        .cornerRadius(16)
        .shadow(color: statusColor.opacity(0.3), radius: 10, x: 0, y: 5)
    }

    private var statusIcon: String {
        switch status {
        case .disconnected:
            return "poweroff"
        case .connecting:
            return "arrow.triangle.2.circlepath"
        case .connected:
            return "checkmark.circle.fill"
        case .error:
            return "exclamationmark.triangle.fill"
        }
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

    private var statusTitle: String {
        switch status {
        case .disconnected:
            return "Disconnected"
        case .connecting:
            return "Connecting..."
        case .connected:
            return "Connected"
        case .error:
            return "Connection Error"
        }
    }

    private var statusSubtitle: String {
        switch status {
        case .disconnected:
            return "Connect to remote system to begin"
        case .connecting:
            return "Establishing secure connection..."
        case .connected:
            return "Connected to remote system"
        case .error:
            return "Failed to connect. Check your settings."
        }
    }
}

// MARK: - System Stats View

struct SystemStatsView: View {
    let systemInfo: SystemInfo

    var body: some View {
        VStack(spacing: 16) {
            Text("System Information")
                .font(.headline)
                .frame(maxWidth: .infinity, alignment: .leading)

            // CPU Usage
            StatRow(
                label: "CPU Usage",
                value: String(format: "%.1f%%", systemInfo.cpuUsage),
                icon: "cpu"
            )

            // Memory Usage
            StatRow(
                label: "Memory Usage",
                value: String(format: "%.1f%%", systemInfo.memoryUsage),
                icon: "memorychip"
            )

            // Version
            StatRow(
                label: "Version",
                value: systemInfo.version,
                icon: "info.circle"
            )

            // Uptime
            StatRow(
                label: "Uptime",
                value: systemInfo.uptime,
                icon: "clock"
            )
        }
        .background(Color(.systemBackground))
        .cornerRadius(16)
        .shadow(color: .black.opacity(0.1), radius: 5, x: 0, y: 3)
    }
}

// MARK: - Stat Row

struct StatRow: View {
    let label: String
    let value: String
    let icon: String

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: icon)
                .foregroundStyle(.blue)
                .frame(width: 30)

            Text(label)
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Spacer()

            Text(value)
                .font(.subheadline)
                .fontWeight(.semibold)
        }
        .padding()
        .background(Color(.secondarySystemBackground))
        .cornerRadius(10)
    }
}

// MARK: - Quick Actions View

struct QuickActionsView: View {
    var body: some View {
        VStack(spacing: 12) {
            Text("Quick Actions")
                .font(.headline)
                .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 12) {
                ActionButton(
                    title: "Restart",
                    icon: "arrow.clockwise",
                    color: .orange
                )

                ActionButton(
                    title: "Shutdown",
                    icon: "power",
                    color: .red
                )
            }

            ActionButton(
                title: "Update",
                icon: "arrow.triangle.2.circlepath",
                color: .blue
            )
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(16)
        .shadow(color: .black.opacity(0.1), radius: 5, x: 0, y: 3)
    }
}

struct ActionButton: View {
    let title: String
    let icon: String
    let color: Color

    var body: some View {
        Button(action: {}) {
            HStack(spacing: 8) {
                Image(systemName: icon)
                Text(title)
            }
            .frame(maxWidth: .infinity)
            .padding()
            .background(color.opacity(0.1))
            .foregroundStyle(color)
            .cornerRadius(10)
        }
    }
}

// MARK: - Preview

#Preview {
    DashboardView(connectionStatus: .constant(.disconnected))
}
