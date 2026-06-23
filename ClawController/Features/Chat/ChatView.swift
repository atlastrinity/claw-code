//
//  ChatView.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI
import ComposableArchitecture

struct ChatView: View {
    let store: Store<ChatState, ChatAction>

    var body: some View {
        ZStack {
            Color.background.ignoresSafeArea()

            VStack(spacing: 0) {
                // Connection Status Bar
                ConnectionStatusBar(
                    connectionState: store.scope(state: \.connectionState, action: .none),
                    isStreaming: store.scope(state: \.isStreaming, action: .none)
                )

                // Messages List
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 12) {
                            ForEach(store.scope(state: \.messages, action: .none)) { message in
                                MessageBubbleView(message: message)
                                    .id(message.id)
                            }
                        }
                        .padding()
                    }
                    .onChange(of: store.scope(state: \.messages, action: .none).count) { _ in
                        if let lastMessage = store.scope(state: \.messages, action: .none).last {
                            proxy.scrollTo(lastMessage.id, anchor: .bottom)
                        }
                    }
                }

                // Input Bar
                ChatInputBar(
                    text: store.scope(state: \.inputText, action: .updateInput),
                    isStreaming: store.scope(state: \.isStreaming, action: .none),
                    onSend: { text in
                        store.send(.sendMessage(text))
                    }
                )
            }
        }
        .onAppear {
            store.send(.connect)
        }
    }
}

struct ConnectionStatusBar: View {
    @Bindable var connectionState: ConnectionState
    var isStreaming: Bool

    var body: some View {
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

struct MessageBubbleView: View {
    @Bindable var message: ChatMessage

    var body: some View {
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

struct UserBubble: View {
    @Bindable var message: ChatMessage

    var body: some View {
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

struct AssistantBubble: View {
    @Bindable var message: ChatMessage

    var body: some View {
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

struct ChatInputBar: View {
    @Binding var text: String
    var isStreaming: Bool
    var onSend: (String) -> Void

    @State private var isFocused: Bool = false

    var body: some View {
        HStack(spacing: 12) {
            if isStreaming {
                // Stop button
                Button(action: {
                    // TODO: Stop streaming
                }) {
                    Image(systemName: "stop.circle.fill")
                        .font(.title2)
                        .foregroundColor(.red)
                }
            } else {
                // Send button
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
                // TODO: Attach file
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

#Preview {
    ChatView(
        store: Store(
            initialState: ChatState(),
            reducer: chatReducer,
            environment: .live
        )
    )
}
