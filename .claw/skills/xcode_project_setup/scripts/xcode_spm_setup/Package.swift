// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "xcode_spm_setup_mcp",
    platforms: [
        .macOS(.v13)
    ],
    dependencies: [
        .package(url: "https://github.com/yonaskolb/XcodeGen.git", from: "2.44.0")
    ],
    targets: [
        .executableTarget(
            name: "xcode_spm_setup_mcp",
            dependencies: [
                .product(name: "XcodeGenKit", package: "XcodeGen")
            ],
            path: "Sources",
            sources: ["xcode_spm_setup_mcp.swift"]
        )
    ]
)
