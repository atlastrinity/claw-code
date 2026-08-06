//
//  ConnectionStatusBar.swift
//  ClawController
//
//  Created by AI Assistant on 2026-08-07.
//

import SwiftUI

public struct ConnectionStatusBar: View {
    @Bindable var connectionState: ConnectionState
    var isStreaming: Bool

    public init(connectionState: ConnectionState, isStreaming: Bool) {
        self.connectionState = connectionState
        self.isStreaming = isStreaming
    }

    public var body: some View {
        HStack {
            Image(systemName: connectionIcon)
                .foregroundColor(statusColor)
            Text(statusText)
                .font(.caption)
                .foregroundColor(statusColor)
            if isStreaming {
                ProgressView()
                    .scaleEffect(0.8)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Color.surface)
        .cornerRadius(20)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }

    private var connectionIcon: String {
        switch connectionState {
        case .connected, .authenticating:
            return "checkmark.circle.fill"
        case .connecting, .reconnecting:
            return "arrow.triangle.2.circlepath"
        case .disconnected, .error:
            return "xmark.circle.fill"
        }
    }

    private var statusText: String {
        switch connectionState {
        case .connected:
            return "Connected"
        case .connecting:
            return "Connecting..."
        case .authenticating:
            return "Authenticating..."
        case .reconnecting:
            return "Reconnecting..."
        case .disconnected:
            return "Disconnected"
        case .error:
            return "Error"
        }
    }

    private var statusColor: Color {
        switch connectionState {
        case .connected, .authenticating:
            return .green
        case .connecting, .reconnecting:
            return .orange
        case .disconnected, .error:
            return .red
        }
    }
}
