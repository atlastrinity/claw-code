//
//  ChatView.swift
//  ClawControllerFeature
//
//  Chat view for interactive command control
//

import SwiftUI

public struct ChatView: View {
    @State private var messages: [ChatMessage] = [
        ChatMessage(text: "Привіт! Я готовий до роботи.", isUser: false),
        ChatMessage(text: "Як справи з поточним проектом?", isUser: true)
    ]
    @State private var newMessage: String = ""

    public init() {}

    public var body: some View {
        VStack {
            ScrollView {
                LazyVStack(spacing: 12) {
                    ForEach(messages) { message in
                        ChatBubble(message: message)
                    }
                }
                .padding()
            }

            HStack {
                TextField("Введіть команду...", text: $newMessage)
                    .textFieldStyle(.roundedBorder)
                    .padding(.horizontal)

                Button(action: sendMessage) {
                    Image(systemName: "paperplane.fill")
                        .font(.title2)
                }
                .padding(.trailing)
                .disabled(newMessage.isEmpty)
            }
            .padding(.bottom)
        }
        .navigationTitle("Chat")
    }

    private func sendMessage() {
        let new = ChatMessage(text: newMessage, isUser: true)
        messages.append(new)
        newMessage = ""
        // В майбутньому тут буде логіка відправки команди на сервер
    }
}

struct ChatMessage: Identifiable {
    let id = UUID()
    let text: String
    let isUser: Bool
}

struct ChatBubble: View {
    let message: ChatMessage

    var body: some View {
        HStack {
            if message.isUser { Spacer() }
            Text(message.text)
                .padding(10)
                .background(message.isUser ? Color.blue : Color.gray.opacity(0.2))
                .foregroundColor(message.isUser ? .white : .primary)
                .cornerRadius(12)
            if !message.isUser { Spacer() }
        }
    }
}

#Preview {
    ChatView()
}
