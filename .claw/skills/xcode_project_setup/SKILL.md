---
name: xcode-project-setup
description: Manage iOS project dependencies using XcodeGenKit (Swift API) and MCP integration. Replaces Ruby gem approach with clean Swift implementation.
compatibility: Requires Swift 5.9+ and XcodeGenKit.
---

# Xcode Project Setup

## ⛔️ CRITICAL RULES & ENVIRONMENT CHECKS

Before performing any Xcode setup or file manipulation, you **MUST** adhere to the following rules. A hefty fee will be applied if you violate them.

### 1. The Anti-Ruby Mandate (CRITICAL)
You are **strictly forbidden** from using Ruby, Rails, or any Ruby gems (including the `xcodeproj` gem). Under no circumstances may you write or execute Ruby scripts.

**WHY?** Ruby gems are:
- ❌ Not maintainable in Swift ecosystem
- ❌ Hidden behind Swift imports (bridging headers)
- ❌ Prone to version conflicts
- ❌ Hard to debug and maintain

**✅ SOLUTION:** Use **XcodeGenKit** (Swift API) for all project manipulations.

### 2. Modern Xcode Folder Synchronization
Modern Xcode projects support folder synchronization. When adding new source code (`.swift`) or resource files, simply write them to the correct directory on disk. They will be automatically included in the Xcode project. **Never manually modify the `.pbxproj` file to add files.**

### 3. Allowed Scripting Languages
If you absolutely must write a script to manipulate the project environment (e.g., configuring SPM packages beyond what the provided scripts do), you **must use Swift**. Only as an absolute last resort, if Swift is completely unviable, may you use Node.js or TypeScript.

### 4. Toolchain Verification
Because this skill relies entirely on native Swift scripts, you must verify the environment:
- Run `swift --version` before proceeding.
- If the Swift command is not found, you must stop and recommend the user install the Swift toolchain (e.g., via `xcode-select --install` on macOS), or ask if you can attempt to install it for them. Do not attempt to proceed without Swift.

### 5. Mandatory Linker Flags for Static Frameworks (Firebase)
When setting up SPM dependencies that heavily rely on internal Objective-C categories and `+load` methods (such as the Firebase iOS SDK suite), the Apple linker will aggressively strip these methods out if they are linked statically.

This causes fatal runtime crashes (e.g., `FirebaseAuth/Auth.swift:167: Fatal error: Unexpectedly found nil`).

**The scripts automatically inject the `-ObjC` flag to `OTHER_LDFLAGS` when adding Firebase products.** However, you should still verify it is present in the build settings if you encounter issues.
- Failing to include this flag when adding Firebase dependencies is a critical error.

---

## Empty Directory Workflow

If you are asked to build an iOS app or configure Xcode dependencies but **no `.xcodeproj` or `.xcworkspace` exists**, you MUST ask the user to create the project first:

**"No Xcode project found in this directory. Please create an empty Xcode project manually and let me know when you are ready to proceed."**

Wait for the user to confirm they have created the `.xcodeproj` via Xcode, then proceed with the Standard Xcode Workflow below.

---

## Standard Xcode Workflow

You have **two** powerful Swift scripts available:

### Script 1: `xcode_spm_setup.swift` (Basic)
For simple package additions without MCP integration.

**Usage:**
```bash
swift run --package-path <PATH_TO_SKILL>/scripts/xcode_spm_setup xcode_spm_setup \
  <Path/To/project.yml> \
  <RepoURL> \
  <VersionRequirement> \
  [--plist <Optional/Path/To/Config.plist>] \
  <Product1> [Product2 ...]
```

**Example:**
```bash
swift run --package-path .claw/skills/xcode_project_setup/scripts/xcode_spm_setup \
  xcode_spm_setup \
  MyApp/project.yml \
  https://github.com/firebase/firebase-ios-sdk \
  11.0.0 \
  --plist MyApp/GoogleService-Info.plist \
  FirebaseCore FirebaseAuth FirebaseFirestore
```

**What it does:**
1. Reads `project.yml`
2. Adds Swift Package dependency
3. Links products to target
4. Adds `-ObjC` linker flag for Firebase
5. Saves updated `project.yml`
6. Prints instructions to run `xcodegen generate`

---

### Script 2: `xcode_spm_setup_mcp.swift` (Advanced)
For complete workflows with MCP integration, documentation fetching, and automated testing.

**Usage:**
```bash
swift run --package-path <PATH_TO_SKILL>/scripts/xcode_spm_setup xcode_spm_setup_mcp \
  <Path/To/project.yml> \
  <RepoURL> \
  <VersionRequirement> \
  [--plist <Optional/Path/To/Config.plist>] \
  <Product1> [Product2 ...]
```

**Example:**
```bash
swift run --package-path .claw/skills/xcode_project_setup/scripts/xcode_spm_setup \
  xcode_spm_setup_mcp \
  MyApp/project.yml \
  https://github.com/firebase/firebase-ios-sdk \
  11.0.0 \
  --plist MyApp/GoogleService-Info.plist \
  FirebaseCore FirebaseAuth FirebaseFirestore
```

**What it does:**
1. ✅ Logs into Firebase MCP server
2. ✅ Deploys Firebase configuration
3. ✅ Fetches Firebase configuration
4. ✅ Adds Swift Package dependency
5. ✅ Links products to target
6. ✅ Validates project configuration
7. ✅ Generates Xcode project
8. ✅ Creates GoogleService-Info.plist
9. ✅ Lists available Firebase services
10. ✅ Provides next steps

---

## CRITICAL: Always Use Latest SDK Version
To ensure access to the latest features and security fixes, always use the most recent version of the Firebase iOS SDK. Check for the latest release version at [https://github.com/firebase/firebase-ios-sdk/releases](https://github.com/firebase/firebase-ios-sdk/releases).
- Use the most recent version number (e.g., `11.x.y`) in your commands instead of hardcoded placeholders.

---

## Understanding the Workflow

### Step 1: Add Package to project.yml
Both scripts modify `project.yml` to add:
- Swift Package dependency in `packages` section
- Product dependencies in `targets` section
- Optional plist file in `sources` section

### Step 2: Run XcodeGen
After the script completes, you **MUST** run:
```bash
xcodegen generate --project <Path/To/project.yml>
```

This generates the updated `.pbxproj` file.

### Step 3: Verify Build
Use `xcode-bridge` MCP to verify compilation:
```bash
# Build the project
xcodebuild -project project.xcodeproj -scheme MyApp build

# Run tests
xcodebuild test -project project.xcodeproj -scheme MyApp
```

### Step 4: Test in Simulator
Use `ios-simulator` MCP to verify the app works:
```bash
# Boot simulator
xcrun simctl boot "iPhone 16 Pro Max"

# Build and install
xcodebuild -project project.xcodeproj -scheme MyApp \
  -destination 'platform=iOS Simulator,name=iPhone 16 Pro Max' \
  build

# Launch app
xcrun simctl launch booted com.example.MyApp
```

---

## Error Handling

### Common Errors and Solutions

#### 1. "No Swift packages defined in project.yml"
**Cause:** `project.yml` doesn't have a `packages` section
**Solution:** Create a basic `project.yml` structure:
```yaml
name: MyApp
options:
  bundleIdPrefix: com.example
  deploymentTarget:
    iOS: "16.0"
targets:
  MyApp:
    type: application
    platform: iOS
    sources: [Sources]
    dependencies: []
packages: []
```

#### 2. "Build failed: No targets found"
**Cause:** Target name doesn't match
**Solution:** Ensure the target name in `project.yml` matches your scheme name

#### 3. "Package already exists"
**Cause:** Package URL already in `packages` section
**Solution:** This is fine - the script will skip adding it again

#### 4. "Invalid package reference"
**Cause:** Product name doesn't match what's available in the package
**Solution:** Check the package's available products at [package URL](https://github.com/username/package)

---

## Integration with MCP Servers

### xcode-bridge MCP
Used for:
- Fetching Apple Developer Documentation
- Building projects
- Running tests
- Getting SDK information

### ios-simulator MCP
Used for:
- Booting simulators
- Installing `.app` bundles
- Launching apps
- Taking screenshots
- UI testing

**Note:** The MCP integration is currently **simulated** in the scripts. To enable real MCP integration:
1. Create an MCP client in the scripts
2. Replace mock functions with real MCP calls
3. Test thoroughly with real Xcode projects

---

## Project.yml Best Practices

### 1. Use project.yml as Single Source of Truth
All project configuration should be in `project.yml`. Never manually edit `.pbxproj`.

### 2. Keep Dependencies Centralized
Add all SPM packages in the `packages` section with clear naming:
```yaml
packages:
  FirebaseAnalytics:
    url: https://github.com/firebase/firebase-ios-sdk
    from: "11.0.0"
  Alamofire:
    url: https://github.com/Alamofire/Alamofire
    from: "5.8.1"
```

### 3. Link Products Explicitly
Always specify which products to link in the `targets` section:
```yaml
targets:
  MyApp:
    dependencies:
      - package: FirebaseAnalytics
        product: FirebaseAnalytics
      - package: Alamofire
        product: Alamofire
```

### 4. Set Deployment Target
Always specify the minimum iOS version:
```yaml
options:
  deploymentTarget:
    iOS: "16.0"
```

---

## Example: Complete Workflow

### Scenario: Add Firebase Authentication

```bash
# 1. Fetch documentation (using xcode-bridge MCP)
# search_documentation query: "Firebase Authentication iOS 26"

# 2. Add package using MCP-integrated script
swift run --package-path .claw/skills/xcode_project_setup/scripts/xcode_spm_setup \
  xcode_spm_setup_mcp \
  MyApp/project.yml \
  https://github.com/firebase/firebase-ios-sdk \
  11.0.0 \
  --plist MyApp/GoogleService-Info.plist \
  FirebaseAuth FirebaseCore

# 3. Generate project
xcodegen generate --project MyApp/project.yml

# 4. Verify build
xcodebuild -project MyApp/project.xcodeproj -scheme MyApp build

# 5. Test in simulator
xcrun simctl boot "iPhone 16 Pro Max"
xcodebuild -project MyApp/project.xcodeproj -scheme MyApp \
  -destination 'platform=iOS Simulator,name=iPhone 16 Pro Max' \
  build
xcrun simctl launch booted com.example.MyApp

# 6. Take screenshot to verify
xcrun simctl io booted screenshot app_screenshot.png
```

---

## Migration from Ruby gem to XcodeGenKit

If you have existing scripts using the Ruby `xcodeproj` gem:

### Old Approach (❌ DON'T USE):
```ruby
require 'xcodeproj'

project = Xcodeproj::Project.open('project.xcodeproj')
target = project.targets.first

# Add package
package = project.add_remote_package(
  'https://github.com/firebase/firebase-ios-sdk',
  'FirebaseCore'
)

# Link product
target.add_dependency(package.product_references.first)

# Save
project.save
```

### New Approach (✅ USE THIS):
```bash
# Use Swift script
swift run --package-path .claw/skills/xcode_project_setup/scripts/xcode_spm_setup \
  xcode_spm_setup \
  project/project.yml \
  https://github.com/firebase/firebase-ios-sdk \
  11.0.0 \
  FirebaseCore

# Then generate
xcodegen generate --project project/project.yml
```

---

## Troubleshooting

### Swift Build Failures
If you see Swift compilation errors:
1. Clear derived data: `rm -rf ~/Library/Developer/Xcode/DerivedData/`
2. Clean build folder: `xcodebuild clean -project project.xcodeproj`
3. Rebuild: `xcodebuild build -project project.xcodeproj -scheme MyApp`

### XcodeGen Generation Errors
If `xcodegen generate` fails:
1. Check `project.yml` syntax (use JSON validator)
2. Ensure all required fields are present
3. Verify file paths are correct
4. Check for YAML syntax errors

### Package Resolution Issues
If SPM can't resolve packages:
1. Check network connection
2. Verify package URL is correct
3. Check version requirement format (e.g., `11.0.0`, `~> 11.0`)
4. Clear SPM cache: `rm -rf ~/Library/Caches/org.swift.swiftpm`

---

## Security Considerations

### 1. Never commit `project.xcworkspace`
The `.xcworkspace` file is generated and should not be committed. It contains build artifacts and should be in `.gitignore`.

### 2. Validate file paths
Always verify file paths before adding them to `project.yml`. Malicious paths could lead to file injection attacks.

### 3. Use environment variables for sensitive data
For API keys and secrets:
```yaml
options:
  env:
    - GOOGLE_SERVICE_PLIST_PATH=${GOOGLE_SERVICE_PLIST_PATH}
```

---

## Resources

- [XcodeGen Documentation](https://github.com/yonaskolb/XcodeGen)
- [Swift Package Manager](https://www.swift.org/package-manager/)
- [Firebase iOS SDK](https://firebase.google.com/docs/ios/setup)
- [Apple Developer Documentation](https://developer.apple.com/documentation/)
