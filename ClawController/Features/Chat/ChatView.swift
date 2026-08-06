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

#Preview {
    ChatView(
        store: Store(
            initialState: ChatState(),
            reducer: chatReducer,
            environment: .live
        )
    )
}

