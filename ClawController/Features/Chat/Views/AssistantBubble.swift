//
//  AssistantBubble.swift
//  ClawController
//
//  Created by AI Assistant on 2026-08-07.
//

import SwiftUI

public struct AssistantBubble: View {
    @Bindable var message: ChatMessage

    public init(message: ChatMessage) {
        self.message = message
    }

    public var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "sparkles")
                .foregroundColor(.secondary)
                .font(.caption)

            VStack(alignment: .leading, spacing: 4) {
                Text(messageContent)
                    .foregroundColor(.primary)
                    .textSelection(.enabled)

                if message.status == .streaming {
                    ProgressView()
                        .scaleEffect(0.5)
                }
            }
        }
        .padding(12)
        .background(Color.surfaceElevated)
        .cornerRadius(16)
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .stroke(Color.primary.opacity(0.1), lineWidth: 1)
        )
    }

    private var messageContent: String {
        switch message.content {
        case .text(let text):
            return text
        case .markdown(let text):
            return text
        case .code(let text, _):
            return text
        case .toolCall(let name, _):
            return "🔧 Tool: \(name)"
        case .error(let text):
            return text
        default:
            return ""
        }
    }
}
