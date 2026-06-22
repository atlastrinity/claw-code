import XCTest
@testable import ClawControllerFeature

final class WebSocketManagerTests: XCTestCase {
    func testConnectionStatus() {
        let manager = WebSocketManager()
        XCTAssertFalse(manager.isConnected)
        manager.connect()
        // Note: Real websocket connection is asynchronous and requires mock.
        // This is a basic test skeleton.
        XCTAssertTrue(manager.isConnected)
    }
}
