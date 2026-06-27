//
//  ClawMcpDemoTests.swift
//  ClawMcpDemoTests
//
//  Created by test
//

import XCTest
@testable import ClawMcpDemo

final class ClawMcpDemoTests: XCTestCase {

    func testIncrement() {
        // Arrange
        let counter = Counter()
        let initialCount = counter.count

        // Act
        counter.increment()

        // Assert
        XCTAssertEqual(counter.count, initialCount + 1, "Increment should increase count by 1")
    }

    func testDecrement() {
        // Arrange
        let counter = Counter()
        let initialCount = counter.count

        // Act
        counter.decrement()

        // Assert
        XCTAssertEqual(counter.count, initialCount - 1, "Decrement should decrease count by 1")
    }
}

// Helper class for testing
struct Counter {
    var count: Int = 0

    mutating func increment() {
        count += 1
    }

    mutating func decrement() {
        count -= 1
    }
}
