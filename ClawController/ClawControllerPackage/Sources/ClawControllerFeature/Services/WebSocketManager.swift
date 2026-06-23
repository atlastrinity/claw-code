import Foundation
import Combine

public final class WebSocketManager: ObservableObject, @unchecked Sendable {
    private var webSocketTask: URLSessionWebSocketTask?
    private let session = URLSession(configuration: .default)
    private let url = URL(string: "ws://localhost:8080/ws")!
    private let queue = DispatchQueue(label: "com.claw.websocket", qos: .userInitiated)

    @Published public var isConnected = false
    @Published public var messages: [String] = []

    public init() {}

    public func connect() async {
        let request = URLRequest(url: url)
        webSocketTask = session.webSocketTask(with: request)
        webSocketTask?.resume()
        isConnected = true
        await receive()
    }

    public func disconnect() {
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        isConnected = false
    }

    @MainActor
    private func receive() {
        webSocketTask?.receive { [weak self] result in
            guard let self = self else { return }

            switch result {
            case .success(let message):
                let text: String?
                switch message {
                case .string(let t): text = t
                case .data(let d): text = String(data: d, encoding: .utf8)
                @unknown default: text = nil
                }

                if let t = text {
                    Task { @MainActor in
                        self.handleMessage(t)
                    }
                }
                Task { @MainActor in
                    self.receive()
                }
            case .failure(let error):
                print("WebSocket error: \(error)")
                Task { @MainActor in
                    self.isConnected = false
                    await self.attemptReconnect()
                }
            }
        }
    }

    public func send(text: String) {
        let message = URLSessionWebSocketTask.Message.string(text)
        webSocketTask?.send(message) { error in
            if let error = error {
                print("WebSocket send error: \(error)")
            }
        }
    }

    @MainActor
    private func handleMessage(_ text: String) {
        self.messages.append(text)
    }

    @MainActor
    private func attemptReconnect() async {
        await self.connect()
    }
}
