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
        ZStack {
            Color.black.edgesIgnoringSafeArea(.all)
            
            VStack {
                Text("CLAW_CODE_TERMINAL")
                    .font(.system(.subheadline, design: .monospaced))
                    .foregroundColor(.green)
                    .padding(.top)
                
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(messages) { message in
                            ChatBubble(message: message)
                        }
                    }
                    .padding()
                }

                HStack {
                    Image(systemName: "terminal.fill")
                        .foregroundColor(.green)
                    
                    TextField("Enter command...", text: $newMessage)
                        .textFieldStyle(.plain)
                        .padding(8)
                        .background(Color.green.opacity(0.1))
                        .foregroundColor(.green)
                        .font(.system(.body, design: .monospaced))
                        .cornerRadius(4)
                        .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.green.opacity(0.5)))
                        .padding(.horizontal)

                    Button(action: sendMessage) {
                        Image(systemName: "paperplane.fill")
                            .font(.title2)
                            .foregroundColor(.green)
                    }
                    .padding(.trailing)
                    .disabled(newMessage.isEmpty)
                }
                .padding(.bottom)
            }
        }
        .navigationTitle("")
        .toolbarBackground(.hidden, for: .navigationBar)
    }

    @Environment(RemoteService.self) private var remoteService: RemoteService

    private func sendMessage() {
        guard !newMessage.isEmpty else { return }

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
                    messages.append(ChatMessage(text: "Result: \(result.message)", isUser: false))
                }
            } catch {
                await MainActor.run {
                    messages.append(ChatMessage(text: "Error: \(error.localizedDescription)", isUser: false))
                }
            }
        }
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
