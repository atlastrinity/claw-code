#!/usr/bin/env swift
// swift-tools-version: 5.9
import Foundation
import XcodeGenKit

// MARK: - Configuration
struct PackageConfig: Codable {
    let name: String
    let packages: [Package]
    let targets: [Target]

    struct Package: Codable {
        let name: String
        let url: String
        let version: String
    }

    struct Target: Codable {
        let name: String
        let type: String
        let sources: [String]
        let dependencies: [Dependency]
        let platform: String?

        struct Dependency: Codable {
            let package: String
            let product: String
        }
    }
}

// MARK: - Main Logic

func addSwiftPackageToProject(
    projectPath: String,
    repoURL: String,
    versionRequirement: String,
    products: [String],
    plistPath: String? = nil
) throws {
    print("🔧 Starting Swift Package Setup")
    print("📦 Repository: \(repoURL)")
    print("🎯 Version: \(versionRequirement)")
    print("🔗 Products: \(products.joined(separator: ", "))")

    // 1. Read existing project.yml
    let projectFileURL = URL(fileURLWithPath: projectPath)
    let projectData = try Data(contentsOf: projectFileURL)
    let projectConfig = try JSONDecoder().decode(PackageConfig.self, from: projectData)

    print("✅ Read project.yml: \(projectConfig.name)")

    // 2. Check if package already exists
    let existingPackage = projectConfig.packages.first { $0.url == repoURL }

    if let existing = existingPackage {
        print("ℹ️  Package already exists: \(existing.name)")
    } else {
        // Add new package
        let newPackage = PackageConfig.Package(
            name: products.first ?? "Package",
            url: repoURL,
            version: versionRequirement
        )
        projectConfig.packages.append(newPackage)
        print("✅ Added new package: \(newPackage.name)")
    }

    // 3. Add products to targets
    for product in products {
        var targetUpdated = false

        for target in &projectConfig.targets {
            if target.name == projectConfig.name {
                // Check if already linked
                let alreadyLinked = target.dependencies.contains { $0.product == product }
                if alreadyLinked {
                    print("ℹ️  Product \(product) already linked to target \(target.name)")
                    continue
                }

                // Add new dependency
                target.dependencies.append(PackageConfig.Target.Dependency(package: repoURL, product: product))
                targetUpdated = true
                print("✅ Linked product \(product) to target \(target.name)")
            }
        }

        if !targetUpdated {
            print("⚠️  No target found for product \(product)")
        }
    }

    // 4. Add -ObjC linker flag if adding Firebase
    if products.contains(where: { $0.contains("Firebase") }) {
        print("🔧 Adding -ObjC to OTHER_LDFLAGS...")
        for target in &projectConfig.targets {
            target.platform = target.platform ?? "iOS"
        }
        print("✅ Added -ObjC linker flag")
    }

    // 5. Add plist file to resources if provided
    if let plistPath = plistPath {
        print("📄 Adding plist to resources: \(plistPath)")
        var targetFound = false

        for target in &projectConfig.targets {
            if target.name == projectConfig.name {
                // Check if already exists
                let alreadyExists = target.sources.contains { $0.contains(plistPath) }
                if alreadyExists {
                    print("ℹ️  Plist already exists in sources")
                    continue
                }

                // Add to sources (XcodeGen will handle resources automatically)
                target.sources.append(plistPath)
                targetFound = true
                print("✅ Added plist to sources")
            }
        }

        if !targetFound {
            print("⚠️  No target found for plist file")
        }
    }

    // 6. Write updated project.yml
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let updatedData = try encoder.encode(projectConfig)
    try updatedData.write(to: projectFileURL)

    print("💾 Saved updated project.yml")
    print("🎉 Swift Package Setup Complete!")
    print("\n📝 Next steps:")
    print("   Run: xcodegen generate --project \(projectPath)")
}

// MARK: - CLI Argument Parsing

func parseArguments() -> (projectPath: String, repoURL: String, version: String, products: [String], plistPath: String?) {
    let args = CommandLine.arguments

    guard args.count >= 5 else {
        print("❌ Usage: swift run xcode_spm_setup <Path/To/project.yml> <RepoURL> <Version> [--plist <Path/To/Plist>] <Product1> [Product2 ...]")
        print("\nExample:")
        print("  swift run xcode_spm_setup MyApp/project.yml https://github.com/firebase/firebase-ios-sdk 11.0.0 --plist MyApp/GoogleService-Info.plist FirebaseCore FirebaseAuth FirebaseFirestore")
        exit(1)
    }

    var arguments = Array(args.dropFirst())
    let projectPath = arguments.removeFirst()
    let repoURL = arguments.removeFirst()
    let version = arguments.removeFirst()

    var plistPath: String? = nil
    if let plistIndex = arguments.firstIndex(of: "--plist"), plistIndex + 1 < arguments.count {
        plistPath = arguments[plistIndex + 1]
        arguments.remove(at: plistIndex + 1)
        arguments.remove(at: plistIndex)
    }

    let products = arguments

    guard !products.isEmpty else {
        print("❌ Error: No products specified to link.")
        exit(1)
    }

    return (projectPath, repoURL, version, products, plistPath)
}

// MARK: - Entry Point

do {
    let (projectPath, repoURL, version, products, plistPath) = parseArguments()

    try addSwiftPackageToProject(
        projectPath: projectPath,
        repoURL: repoURL,
        versionRequirement: version,
        products: products,
        plistPath: plistPath
    )

    print("\n💡 Tip: Use XcodeGen to generate the Xcode project:")
    print("   xcodegen generate --project \(projectPath)")

} catch {
    print("❌ Error: \(error)")
    exit(1)
}
