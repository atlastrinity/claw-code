//
//  MessageBubbleView.swift
//  ClawController
//
//  Created by AI Assistant on 2026-08-07.
//

import SwiftUI

public struct MessageBubbleView: View {
    @Bindable var message: ChatMessage

    public init(message: ChatMessage) {
        self.message = message
    }

    public var body: some View {
        HStack {
            if message.role == .user {
                Spacer()
                UserBubble(message: message)
            } else {
                AssistantBubble(message: message)
                Spacer()
            }
        }
    }
}
