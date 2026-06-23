#!/usr/bin/env swift
import Foundation

// Arguments:
// 1. Project path (e.g., "ClawController.xcodeproj")
// 2. Package path (e.g., "../ClawControllerPackage")

guard CommandLine.arguments.count >= 3 else {
    print("Usage: \(CommandLine.arguments[0]) <project_path> <package_path>")
    exit(1)
}

let projectPath = CommandLine.arguments[1]
let packagePath = CommandLine.arguments[2]

let projectFileURL = URL(fileURLWithPath: projectPath).appendingPathComponent("project.pbxproj")
guard FileManager.default.fileExists(atPath: projectFileURL.path) else {
    print("Error: Project file not found at \(projectFileURL.path)")
    exit(1)
}

var projectContent = try String(contentsOfFile: projectFileURL.path, encoding: .utf8)

// Calculate relative path from project to package
let projectDir = URL(fileURLWithPath: projectPath).deletingLastPathComponent()
let packageURL = URL(fileURLWithPath: packagePath)
let relativePackagePath = packageURL.path.replacingOccurrences(of: projectDir.path + "/", with: "")

print("Project directory: \(projectDir.path)")
print("Package path: \(packagePath)")
print("Relative package path: \(relativePackagePath)")

// Generate unique IDs
let uuid1 = UUID().uuidString.lowercased()
let uuid2 = UUID().uuidString.lowercased()
let uuid3 = UUID().uuidString.lowercased()

// Find packageReferences section - uses 3 tabs
let packageReferencesPattern = "\t\t\tpackageReferences = (\n\t\t\t\t);"

if projectContent.contains(packageReferencesPattern) {
    let replacement = """
		packageReferences = (
			\(uuid1) /* ClawControllerPackage */;
		);
	"""
    projectContent = projectContent.replacingOccurrences(of: packageReferencesPattern, with: replacement)

    // Add local package reference
    let localPackagePattern = "/* Begin XCLocalSwiftPackageReferenceSection */"
    let localPackageReplacement = """
	/* Begin XCLocalSwiftPackageReferenceSection */
		\(uuid2) /* ClawControllerPackage */ = {
			isa = XCLocalSwiftPackageReference;
			path = \(relativePackagePath);
		};
/* End XCLocalSwiftPackageReferenceSection */
"""
    projectContent = projectContent.replacingOccurrences(of: localPackagePattern, with: localPackageReplacement, options: [.caseInsensitive, .regularExpression])

    // Add package product dependency
    let packageProductPattern = "/* Begin XCSwiftPackageProductDependency section */"
    let packageProductReplacement = """
	/* Begin XCSwiftPackageProductDependency section */
		\(uuid3) /* ClawControllerFeature */ = {
			isa = XCSwiftPackageProductDependency;
			package = \(uuid2) /* ClawControllerPackage */;
			productName = ClawControllerFeature;
		};
/* End XCSwiftPackageProductDependency section */
"""
    projectContent = projectContent.replacingOccurrences(of: packageProductPattern, with: packageProductReplacement, options: [.caseInsensitive, .regularExpression])

    try projectContent.write(to: projectFileURL, atomically: true, encoding: .utf8)
    print("Successfully added local package reference to project")
} else {
    print("Error: Could not find packageReferences section in project file")
    exit(1)
}
