// swift-tools-version:5.9
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "ClawMcpDemo",
    platforms: [
        .iOS(.v17)
    ],
    products: [
        .executable(
            name: "ClawMcpDemo",
            targets: ["ClawMcpDemo"]
        )
    ],
    targets: [
        .executableTarget(
            name: "ClawMcpDemo",
            path: "."
        )
    ]
)
