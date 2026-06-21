//
//  ConnectionStatusView.swift
//  ClawControllerUI
//
//  SwiftUI view for displaying connection status and system information
//

import SwiftUI

struct ConnectionStatusView: View {
    @Bindable var state: RemoteControllerState

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Connection Status Header
            statusHeader

            // Connection Info Card
            if state.isConnected {
                connectionInfoCard
            } else {
                disconnectedCard
            }

            // System Info Card
            systemInfoCard
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(color: Color.black.opacity(0.1), radius: 4, x: 0, y: 2)
    }

    // MARK: - Connection Status Header

    private var statusHeader: some View {
        HStack {
            Image(systemName: statusIcon)
                .font(.system(size: 24))
                .foregroundColor(statusColor)
            Text(statusTitle)
                .font(.headline)
            Spacer()
            if state.isConnected {
                ProgressView()
                    .scaleEffect(0.8)
            }
        }
    }

    private var statusTitle: String {
        switch state.connectionStatus {
        case .disconnected:
            return "Disconnected"
        case .connecting:
            return "Connecting..."
        case .connected:
            return "Connected"
        }
    }

    private var statusIcon: String {
        switch state.connectionStatus {
        case .disconnected:
            return "network.slash"
        case .connecting:
            return "arrow.triangle.2.circlepath"
        case .connected:
            return "checkmark.circle.fill"
        }
    }

    private var statusColor: Color {
        switch state.connectionStatus {
        case .disconnected:
            return .red
        case .connecting:
            return .orange
        case .connected:
            return .green
        }
    }

    // MARK: - Connection Info Card

    private var connectionInfoCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Connected to", systemImage: "server")
            Text("\(state.connectionConfig.host):\(state.connectionConfig.port)")
                .font(.title2)
                .fontWeight(.semibold)
                .foregroundColor(.primary)

            HStack {
                Image(systemName: "cpu")
                Text("CPU: \(String(format: "%.1f", state.systemInfo.cpuUsage))%")
            }
            .font(.subheadline)

            HStack {
                Image(systemName: "memorychip")
                Text("Memory: \(String(format: "%.1f", state.systemInfo.memoryUsage))%")
            }
            .font(.subheadline)

            HStack {
                Image(systemName: "internaldrive")
                Text("Disk: \(state.systemInfo.availableDiskSpace.formatted()) MB")
            }
            .font(.subheadline)
        }
        .padding()
        .background(Color(.secondarySystemBackground))
        .cornerRadius(8)
    }

    // MARK: - Disconnected Card

    private var disconnectedCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Not Connected", systemImage: "exclamationmark.triangle")
                .font(.headline)
                .foregroundColor(.orange)

            Text("Please configure connection settings and connect to start remote control")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .padding()
        .background(Color(.secondarySystemBackground))
        .cornerRadius(8)
    }

    // MARK: - System Info Card

    private var systemInfoCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("System Information")
                    .font(.headline)
                Spacer()
                Button(action: {
                    Task {
                        try? await state.refreshStatus()
                    }
                }) {
                    Image(systemName: "arrow.clockwise")
                        .font(.caption)
                }
                .buttonStyle(.borderless)
            }

            if state.isConnected {
                VStack(alignment: .leading, spacing: 6) {
                    infoRow("OS", state.systemInfo.osVersion)
                    infoRow("Uptime", state.systemInfo.uptime)
                    infoRow("Hostname", state.systemInfo.hostname)
                    infoRow("Last Updated", state.systemInfo.lastUpdated, compact: true)
                }
            } else {
                Text("Connect to see system information")
                    .font(.subheadline)
                    .foregroundColor(.secondary)
            }
        }
        .padding()
        .background(Color(.secondarySystemBackground))
        .cornerRadius(8)
    }

    private func infoRow(_ label: String, _ value: String, compact: Bool = false) -> some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundColor(.secondary)
                .frame(width: 80, alignment: .leading)
            Text(value)
                .font(compact ? .caption : .subheadline)
                .foregroundColor(.primary)
            Spacer()
        }
    }
}

// MARK: - Preview

struct ConnectionStatusView_Previews: PreviewProvider {
    static var previews: some View {
        ConnectionStatusView(state: RemoteControllerState())
            .padding()
            .previewLayout(.sizeThatFits)
    }
}
