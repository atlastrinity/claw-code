import Foundation
import Combine

public class WebSocketManager: ObservableObject {
    private var webSocketTask: URLSessionWebSocketTask?
    private let session = URLSession(configuration: .default)
    private let url = URL(string: "ws://localhost:8080/ws")!

    @Published public var isConnected = false
    @Published public var messages: [String] = []
    
    public init() {}
    
    public func connect() {
        let request = URLRequest(url: url)
        webSocketTask = session.webSocketTask(with: request)
        webSocketTask?.resume()
        isConnected = true
        receive()
    }
    
    public func disconnect() {
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        isConnected = false
    }
    
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
                self.receive()
            case .failure(let error):
                print("WebSocket error: \(error)")
                Task { @MainActor in
                    self.isConnected = false
                    self.attemptReconnect()
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
    private func attemptReconnect() {
        self.connect()
    }
}
