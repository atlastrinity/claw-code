import Foundation
import XcodeProj

let projectPath = URL(fileURLWithPath: "ClawMcpDemo.xcodeproj")
let project = XcodeProj(path: projectPath)

// Create project structure
let mainGroup = project.mainGroup
let appGroup = PBXGroup()
let testGroup = PBXGroup()

appGroup.name = "App"
testGroup.name = "ClawMcpDemoTests"

// Add files to groups
let appFiles = [
    "ClawMcpDemoApp.swift",
    "ContentView.swift"
]

let testFiles = [
    "ClawMcpDemoTests.swift"
]

for fileName in appFiles {
    let fileRef = PBXFileReference(
        path: fileName,
        sourceTree: "SOURCE_ROOT",
        lastKnownFileType: "sourcecode.swift"
    )
    appGroup.children.append(fileRef)
}

for fileName in testFiles {
    let fileRef = PBXFileReference(
        path: fileName,
        sourceTree: "SOURCE_ROOT",
        lastKnownFileType: "sourcecode.swift"
    )
    testGroup.children.append(fileRef)
}

mainGroup.children = [appGroup, testGroup]

// Add build phases
let appTarget = PBXNativeTarget(
    name: "ClawMcpDemo",
    platform: .iOS,
    productType: .application,
    productName: "ClawMcpDemo"
)

let testTarget = PBXNativeTarget(
    name: "ClawMcpDemoTests",
    platform: .iOS,
    productType: .unitTestBundle,
    productName: "ClawMcpDemoTests"
)

appTarget.buildPhases = [PBXBuildPhase()]
testTarget.buildPhases = [PBXBuildPhase()]

project.addTarget(appTarget)
project.addTarget(testTarget)

// Save project
try project.write(path: projectPath)
print("Xcode project created successfully")
