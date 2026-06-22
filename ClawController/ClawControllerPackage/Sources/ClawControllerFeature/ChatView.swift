//
//  ChatView.swift
//  ClawControllerFeature
//
//  Chat view for interactive command control
//

import SwiftUI

public struct ChatView: View {
    @Environment(RemoteService.self) private var remoteService
    @State private var messages: [RemoteControllerState.ChatMessage] = [
        RemoteControllerState.ChatMessage(text: "Привіт! Я готовий до роботи.", isUser: false),
        RemoteControllerState.ChatMessage(text: "Я Claw Controller - готовий до роботи з MCP tool integration", isUser: false)
    ]
    @State private var newMessage: String = ""

    public init() {}

    public var body: some View {
        HackerTheme.styledView {
            VStack(spacing: 0) {
                // Header
                chatHeader

                // Messages
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(messages) { message in
                            ChatBubble(message: message)
                        }
                    }
                    .padding(.vertical, 12)
                }
                .scrollContentBackground(.hidden)

                // Input
                chatInput
            }
        }
        .navigationTitle("")
        .toolbarBackground(.hidden, for: .navigationBar)
    }

    // MARK: - Chat Header

    private var chatHeader: some View {
        HStack {
            Image(systemName: "message.circle.fill")
                .font(.system(size: 20))
                .foregroundColor(HackerTheme.accentColor)

            Text("AI Chat")
                .font(.headline)
                .foregroundColor(HackerTheme.accentColor)

            Spacer()

            if messages.count > 0 {
                Button(action: {
                    clearChat()
                }) {
                    Text("Clear")
                        .font(.caption)
                        .foregroundColor(HackerTheme.accentColor)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(HackerTheme.backgroundColor)
    }

    // MARK: - Chat Input

    private var chatInput: some View {
        HStack(spacing: 8) {
            Image(systemName: "terminal.fill")
                .foregroundColor(HackerTheme.accentColor)
                .font(.system(size: 14))

            TextField("Type a message...", text: $newMessage)
                .textFieldStyle(.plain)
                .autocapitalization(.sentences)
                .font(.system(.body, design: .monospaced))
                .padding(8)
                .background(HackerTheme.backgroundColor)
                .overlay(
                    RoundedRectangle(cornerRadius: 0)
                        .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
                )
                .padding(.horizontal)

            Button(action: sendMessage) {
                Image(systemName: "arrow.up.circle.fill")
                    .font(.system(size: 20))
                    .foregroundColor(HackerTheme.accentColor)
            }
            .buttonStyle(.plain)
            .disabled(newMessage.isEmpty || !remoteService.isConnected)
        }
        .padding(8)
        .background(HackerTheme.backgroundColor)
        .overlay(
            RoundedRectangle(cornerRadius: 0)
                .stroke(HackerTheme.panelBorderColor, lineWidth: 1)
        )
        .padding(.horizontal)
        .padding(.vertical, 8)
    }

    // MARK: - Send Message

    private func sendMessage() {
        guard !newMessage.isEmpty else { return }
        guard remoteService.isConnected else {
            print("Not connected to remote service")
            return
        }

        let text = newMessage
        let new = ChatMessage(text: text, isUser: true)
        messages.append(new)
        newMessage = ""

        // Capturing the remoteService locally to avoid data race in Task
        let service = remoteService
        Task {
            do {
                let result = try await service.executeMCPTool("chatCommand", arguments: ["text": text])
                await MainActor.run {
                    messages.append(RemoteControllerState.ChatMessage(text: result.message, isUser: false))
                }
            } catch {
                await MainActor.run {
                    messages.append(RemoteControllerState.ChatMessage(text: "Error: \(error.localizedDescription)", isUser: false))
                }
            }
        }
    }

    // MARK: - Clear Chat

    private func clearChat() {
        messages = [
            RemoteControllerState.ChatMessage(text: "Chat cleared. Ready for new messages.", isUser: false)
        ]
    }
}

// MARK: - Chat Bubble

struct ChatBubble: View {
    let message: RemoteControllerState.ChatMessage

    var body: some View {
        HStack {
            if message.isUser { Spacer() }

            VStack(alignment: message.isUser ? .trailing : .leading, spacing: 4) {
                Text(message.text)
                    .font(.system(.body, design: .monospaced))
                    .foregroundColor(message.isUser ? HackerTheme.textColor : HackerTheme.accentColor)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(message.isUser ? HackerTheme.accentColor.opacity(0.2) : HackerTheme.backgroundColor)
                    .overlay(
                        RoundedRectangle(cornerRadius: 0)
                            .stroke(message.isUser ? HackerTheme.panelBorderColor : HackerTheme.panelBorderColor, lineWidth: 1)
                    )
                    .cornerRadius(0)

                Text(message.timestamp, format: .dateTime.hour().minute().second())
                    .font(.caption2)
                    .foregroundColor(HackerTheme.accentColor.opacity(0.7))
            }
            .padding(.horizontal, 8)

            if !message.isUser { Spacer() }
        }
    }
}

// MARK: - Preview

#Preview {
    ChatView()
}
