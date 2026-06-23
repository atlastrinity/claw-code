//
//  ChatReducer.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation
import ComposableArchitecture

public enum ChatAction {
    case connect
    case connectResponse(Result<Void, Error>)
    case disconnect
    case sendMessage(String)
    case receiveEvent(ServerEvent)
    case updateInput(String)
    case toggleSearch
    case scrollToBottom
}

public enum ChatEnvironment {
    public struct WebSocketClient {
        public let url: URL
        public init(url: URL) {
            self.url = url
        }
    }

    public var websocketClient: WebSocketClient
    public var logger: Logger

    public static var live = ChatEnvironment(
        websocketClient: .init(url: .init(string: "ws://localhost:8080/ws")!),
        logger: .init(subsystem: "com.clawcode.controller", category: "Chat")
    )
}

public struct ChatState: Equatable {
    public var connectionState: ConnectionState
    public var messages: [ChatMessage]
    public var inputText: String
    public var isStreaming: Bool
    public var searchQuery: String
    public var isSearchVisible: Bool
    public var currentSession: AgentSession?

    public init(
        connectionState: ConnectionState = .disconnected(reason: .manual),
        messages: [ChatMessage] = [],
        inputText: String = "",
        isStreaming: Bool = false,
        searchQuery: String = "",
        isSearchVisible: Bool = false,
        currentSession: AgentSession? = nil
    ) {
        self.connectionState = connectionState
        self.messages = messages
        self.inputText = inputText
        self.isStreaming = isStreaming
        self.searchQuery = searchQuery
        self.isSearchVisible = isSearchVisible
        self.currentSession = currentSession
    }
}

public let chatReducer = Reducer<ChatState, ChatAction, ChatEnvironment> { state, action, environment in
    switch action {
    case .connect:
        guard case .disconnected = state.connectionState else { return .none }
        state.connectionState = .connecting(attempt: 1)
        state.messages.append(ChatMessage(
            role: .system,
            content: .text("Connecting to claw-analog...")
        ))
        return .none

    case let .connectResponse(.success):
        state.connectionState = .connected(since: Date())
        state.messages.append(ChatMessage(
            role: .system,
            content: .text("Connected successfully!")
        ))
        return .none

    case let .connectResponse(.failure(error)):
        state.connectionState = .error(.connectionFailed(error.localizedDescription))
        state.messages.append(ChatMessage(
            role: .system,
            content: .error("Failed to connect: \(error.localizedDescription)")
        ))
        return .none

    case .disconnect:
        // TODO: Implement actual disconnect logic
        state.connectionState = .disconnected(reason: .manual)
        state.messages.append(ChatMessage(
            role: .system,
            content: .text("Disconnected")
        ))
        return .none

    case let .sendMessage(text):
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return .none }
        guard case .connected = state.connectionState else { return .none }

        let messageId = UUID()
        state.messages.append(ChatMessage(
            id: messageId,
            role: .user,
            content: .text(text),
            status: .sending
        ))

        state.inputText = ""

        // TODO: Send to WebSocket
        state.messages[state.messages.count - 1].status = .sent

        return .none

    case let .receiveEvent(event):
        switch event {
        case .runStarted(let payload):
            if state.currentSession == nil {
                state.currentSession = AgentSession(
                    workspace: payload.workspace,
                    model: payload.model,
                    permission: payload.permission,
                    preset: payload.preset,
                    ragEnabled: payload.ragEnabled
                )
            }

        case .turnStarted(let turn):
            // TODO: Update turn counter

        case .textDelta(let delta):
            if state.isStreaming {
                if let lastMessage = state.messages.last,
                   case .text(var text) = lastMessage.content {
                    state.messages[state.messages.count - 1].content = .text(text + delta)
                }
            } else {
                state.isStreaming = true
                state.messages.append(ChatMessage(
                    role: .assistant,
                    content: .text(delta),
                    status: .streaming
                ))
            }

        case .turnCompleted(let turn):
            state.isStreaming = false
            if let lastMessage = state.messages.last {
                state.messages[state.messages.count - 1].status = .completed
            }

        case .toolResult(let result):
            // TODO: Handle tool results

        case .runEnded(let ok):
            if ok {
                state.messages.append(ChatMessage(
                    role: .system,
                    content: .text("Run completed successfully")
                ))
            } else {
                state.messages.append(ChatMessage(
                    role: .system,
                    content: .error("Run failed")
                ))
            }

        case .error(let error):
            state.messages.append(ChatMessage(
                role: .system,
                content: .error(error.message)
            ))
        }

        return .none

    case let .updateInput(text):
        state.inputText = text
        return .none

    case .toggleSearch:
        state.isSearchVisible.toggle()
        return .none

    case .scrollToBottom:
        // TODO: Implement scroll to bottom
        return .none
    }
}
