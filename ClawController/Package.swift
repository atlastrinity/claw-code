// swift-tools-version: 6.2
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "ClawController",
    platforms: [.iOS(.v17)],
    products: [
        .executable(
            name: "ClawController",
            targets: ["ClawController"]
        ),
    ],
    dependencies: [
        .package(path: "./ClawControllerPackage")
    ],
    targets: [
        .target(
            name: "ClawController",
            dependencies: ["ClawControllerFeature"],
            path: "Sources/ClawController"
        ),
    ]
)
