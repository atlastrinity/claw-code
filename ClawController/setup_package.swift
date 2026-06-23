#!/usr/bin/env swift
import Foundation

// Xcode Project Setup Script
// This script adds a local Swift Package dependency to an Xcode project

struct ScriptArguments {
    let projectPath: String
    let packageName: String
    let packagePath: String
    let products: [String]
    let workspacePath: String?
}

func main() {
    // Parse arguments
    var args = CommandLine.arguments
    guard args.count >= 6 else {
        print("Usage: \(args[0]) <projectPath> <packageName> <packagePath> <product1> [product2 ...]")
        print("Example: \(args[0]) ClawController.xcodeproj ClawControllerFeature ../ClawControllerPackage ClawControllerFeature")
        exit(1)
    }

    let projectPath = args[1]
    let packageName = args[2]
    let packagePath = args[3]
    let products = Array(args[4...])
    let workspacePath = args.count > 6 ? args[6] : nil

    print("🔧 Xcode Project Package Setup")
    print("  Project: \(projectPath)")
    print("  Package: \(packagePath)")
    print("  Products: \(products.joined(separator: ", "))")

    // Read the project file
    let projectFileURL = URL(fileURLWithPath: projectPath)
    var projectContents = try String(contentsOfFile: projectFileURL.path, encoding: .utf8)

    // Generate unique IDs for the package reference and target
    let packageReferenceID = generateID()
    let packageTargetID = generateID()
    let buildFileID = generateID()
    let frameworkBuildPhaseID = generateID()

    // Create the package reference section
    let packageReferenceSection = """
    /* Begin XCRemoteSwiftPackageReference section */
    \t\(packageReferenceID) /* XCRemoteSwiftPackageReference */ = {
    \t\tisa = XCRemoteSwiftPackageReference;
    \t\trepositoryURL = "\(packagePath)";
    \t\tsystemVersion = 1;
    \t};
    /* End XCRemoteSwiftPackageReference section */
    """

    // Create the package dependency section
    let packageDependencySection = """
    /* Begin XCRemoteSwiftPackageDependency section */
    \t\(generateID()) /* XCRemoteSwiftPackageDependency */ = {
    \t\tisa = XCRemoteSwiftPackageDependency;
    \t\tpackageRef = \(packageReferenceID) /* XCRemoteSwiftPackageReference */;
    \t};
    /* End XCRemoteSwiftPackageDependency section */
    """

    // Insert sections into the project file
    // We'll insert after the "PBXProject" section
    let insertAfterProject = projectContents.range(of: "/* Begin PBXProject section */", options: .caseInsensitive)
    if let insertPoint = insertAfterProject {
        let endOfSection = projectContents.index(after: insertPoint.upperBound)
        let modifiedContent = String(projectContents[..<endOfSection]) +
            "\n" + packageReferenceSection +
            "\n" + packageDependencySection +
            "\n" + String(projectContents[endOfSection...])
        projectContents = modifiedContent
    }

    // Add the package target
    let packageTargetSection = """
    /* Begin XCBuildConfiguration section */
    \t\(packageTargetID) /* \(packageName) */ = {
    \t\tisa = PBXNativeTarget;
    \t\tbuildConfigurationList = \(generateID()) /* Build configuration list for PBXNativeTarget "\(packageName)" */;
    \t\tbuildPhases = (
    \t\t\t\(frameworkBuildPhaseID) /* Frameworks */,
    \t\t);
    \t\tbuildRules = (
    \t\t);
    \t\tdependencies = (
    \t\t);
    \t\tname = \(packageName);
    \t\tproductName = \(packageName);
    \t\tproductReference = \(generateID()) /* \(packageName) */;
    \t\tproductType = "com.apple.product-type.library.static";
    \t};
    /* End XCBuildConfiguration section */
    """

    // Insert after the package dependency section
    let insertAfterPackage = projectContents.range(of: "/* End XCRemoteSwiftPackageDependency section */", options: .caseInsensitive)
    if let insertPoint = insertAfterPackage {
        let endOfSection = projectContents.index(after: insertPoint.upperBound)
        let modifiedContent = String(projectContents[..<endOfSection]) +
            "\n" + packageTargetSection +
            "\n" + String(projectContents[endOfSection...])
        projectContents = modifiedContent
    }

    // Write back to project file
    try projectContents.write(to: projectFileURL, encoding: .utf8)
    print("✅ Successfully added package reference to project")
}

func generateID() -> String {
    let chars = Array("0123456789ABCDEF")
    var id = ""
    for _ in 0..<24 {
        id += String(chars[Int.random(in: 0..<16)])
    }
    return id
}

main()
