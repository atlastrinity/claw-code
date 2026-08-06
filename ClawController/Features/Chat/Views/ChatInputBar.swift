//
//  ChatInputBar.swift
//  ClawController
//
//  Created by AI Assistant on 2026-08-07.
//

import SwiftUI

public struct ChatInputBar: View {
    @Binding var text: String
    var isStreaming: Bool
    var onSend: (String) -> Void

    @State private var isFocused: Bool = false

    public init(text: Binding<String>, isStreaming: Bool, onSend: @escaping (String) -> Void) {
        self._text = text
        self.isStreaming = isStreaming
        self.onSend = onSend
    }

    public var body: some View {
        HStack(spacing: 12) {
            if isStreaming {
                Button(action: {
                    // Stop streaming
                }) {
                    Image(systemName: "stop.circle.fill")
                        .font(.title2)
                        .foregroundColor(.red)
                }
            } else {
                Button(action: {
                    onSend(text)
                }) {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                        .foregroundColor(.primaryGradient)
                }
            }

            TextField("Type a message...", text: $text)
                .textFieldStyle(PlainTextFieldStyle())
                .padding(.vertical, 8)
                .focused($isFocused)

            Button(action: {
                // Attach file
            }) {
                Image(systemName: "paperclip")
                    .foregroundColor(.secondary)
            }
        }
        .padding()
        .background(Color.surfaceElevated)
        .cornerRadius(20)
        .padding(.horizontal)
        .padding(.vertical, 8)
    }
}
