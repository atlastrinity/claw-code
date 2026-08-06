//
//  UserBubble.swift
//  ClawController
//
//  Created by AI Assistant on 2026-08-07.
//

import SwiftUI

public struct UserBubble: View {
    @Bindable var message: ChatMessage

    public init(message: ChatMessage) {
        self.message = message
    }

    public var body: some View {
        Text(messageContent)
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(Color.primaryGradient)
            .foregroundColor(.white)
            .cornerRadius(20)
            .frame(maxWidth: .infinity, alignment: .trailing)
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
