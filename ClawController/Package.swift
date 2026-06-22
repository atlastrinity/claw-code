// swift-tools-version: 6.2
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "ClawController",
    products: [
        .executable(
            name: "ClawController",
            targets: ["ClawController"]
        ),
        .library(
            name: "ClawControllerFeature",
            targets: ["ClawControllerFeature"]
        ),
    ],
    targets: [
        // Targets are the basic building blocks of a package, defining a module or a test suite.
        // Targets can depend on other targets in this package and products from dependencies.
        .target(
            name: "ClawController",
            dependencies: ["ClawControllerFeature"]
        ),
        .target(
            name: "ClawControllerFeature",
            path: "ClawControllerPackage/Sources/ClawControllerFeature"
        ),
    ]
)
