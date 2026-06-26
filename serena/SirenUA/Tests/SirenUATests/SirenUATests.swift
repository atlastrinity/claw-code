import XCTest
@testable import SirenUA

final class SirenUATests: XCTestCase {
    func testAlertParsing() throws {
        let json = """
        {
            "alerts": [
                {
                    "id": "1",
                    "region": "Kyiv",
                    "active": true,
                    "type": "air_raid",
                    "changed": "2026-06-26T12:00:00Z"
                }
            ]
        }
        """.data(using: .utf8)!
        
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let response = try decoder.decode(AlertResponse.self, from: json)
        
        XCTAssertEqual(response.alerts.count, 1)
        XCTAssertEqual(response.alerts[0].region, "Kyiv")
        XCTAssertTrue(response.alerts[0].active)
    }
}
