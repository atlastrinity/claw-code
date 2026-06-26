// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "SirenUA",
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "SirenUA", targets: ["SirenUA"]),
    ],
    targets: [
        .target(name: "SirenUA", dependencies: []),
        .testTarget(name: "SirenUATests", dependencies: ["SirenUA"]),
    ]
)
