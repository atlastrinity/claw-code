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
        HackerTheme.styledView {
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
        }
    }

    // MARK: - Connection Status Header

    private var statusHeader: some View {
        HStack {
            Image(systemName: statusIcon)
                .font(.system(size: 24))
                .foregroundColor(statusColor)
            Text(statusTitle)
                .font(.headline)
                .foregroundColor(HackerTheme.accentColor)
            Spacer()
            if state.isConnected {
                ProgressView()
                    .scaleEffect(0.8)
                    .tint(HackerTheme.accentColor)
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
            return HackerTheme.accentColor
        }
    }

    // MARK: - Connection Info Card

    private var connectionInfoCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Connected to", systemImage: "server")
                .foregroundColor(HackerTheme.accentColor)
            Text("\(state.connectionConfig.host):\(state.connectionConfig.port)")
                .font(.title2)
                .fontWeight(.semibold)
                .foregroundColor(HackerTheme.textColor)

            HStack {
                Image(systemName: "cpu")
                Text("CPU: \(String(format: "%.1f", state.systemInfo.cpuUsage))%")
            }
            .font(.subheadline)
            .foregroundColor(HackerTheme.textColor)

            HStack {
                Image(systemName: "memorychip")
                Text("Memory: \(String(format: "%.1f", state.systemInfo.memoryUsage))%")
            }
            .font(.subheadline)
            .foregroundColor(HackerTheme.textColor)

            HStack {
                Image(systemName: "internaldrive")
                Text("Disk: \(state.systemInfo.availableDiskSpace.formatted()) MB")
            }
            .font(.subheadline)
            .foregroundColor(HackerTheme.textColor)
        }
        .padding()
        .background(HackerTheme.backgroundColor)
        .overlay(
            RoundedRectangle(cornerRadius: 0)
                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
        )
    }

    // MARK: - Disconnected Card

    private var disconnectedCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Not Connected", systemImage: "exclamationmark.triangle")
                .font(.headline)
                .foregroundColor(.red)

            Text("Please configure connection settings and connect to start remote control")
                .font(.subheadline)
                .foregroundColor(HackerTheme.textColor)
        }
        .padding()
        .background(HackerTheme.backgroundColor)
        .overlay(
            RoundedRectangle(cornerRadius: 0)
                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
        )
    }

    // MARK: - System Info Card

    private var systemInfoCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("System Information")
                    .font(.headline)
                    .foregroundColor(HackerTheme.accentColor)
                Spacer()
                Button(action: {
                    Task {
                        try? await state.refreshStatus()
                    }
                }) {
                    Image(systemName: "arrow.clockwise")
                        .font(.caption)
                        .foregroundColor(HackerTheme.accentColor)
                }
                .buttonStyle(.plain)
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
                    .foregroundColor(HackerTheme.textColor)
            }
        }
        .padding()
        .background(HackerTheme.backgroundColor)
        .overlay(
            RoundedRectangle(cornerRadius: 0)
                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
        )
    }

    private func infoRow(_ label: String, _ value: String, compact: Bool = false) -> some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundColor(HackerTheme.accentColor)
                .frame(width: 80, alignment: .leading)
            Text(value)
                .font(compact ? .caption : .subheadline)
                .foregroundColor(HackerTheme.textColor)
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
