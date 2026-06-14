#!/usr/bin/env swift

import Foundation
import Cocoa

// MARK: - Permission Setup Script
class PermissionSetup {
    
    // MARK: - Properties
    private let logger = Logger()
    
    // MARK: - Main Setup Function
    func setupAllPermissions() {
        logger.log("🚀 Starting macOS MCP Server Permission Setup...")
        
        // Check if this is first run
        if isFirstRun() {
            logger.log("📋 First run detected - setting up permissions...")
            setupPermissions()
            createFirstRunMarker()
        } else {
            logger.log("✅ Permissions already configured")
        }
    }
    
    // MARK: - First Run Detection
    private func isFirstRun() -> Bool {
        let markerPath = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".macos_mcp_setup_complete")
        
        return !FileManager.default.fileExists(atPath: markerPath.path)
    }
    
    private func createFirstRunMarker() {
        let markerPath = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".macos_mcp_setup_complete")
        
        do {
            try "".write(to: markerPath, atomically: true, encoding: .utf8)
            logger.log("✅ First run marker created")
        } catch {
            logger.log("❌ Failed to create first run marker: \(error)")
        }
    }
    
    // MARK: - Permission Setup
    private func setupPermissions() {
        // 1. Accessibility Permissions (without user prompt)
        setupAccessibilityPermissions()
        
        // 2. Automation Permissions (with user prompt for Calendar/Reminders)
        setupAutomationPermissions()
        
        // 3. Full Disk Access (without user prompt)
        setupFullDiskAccess()
        
        // 4. Screen Recording (without user prompt)
        setupScreenRecordingPermissions()
        
        // 5. Input Monitoring (without user prompt)
        setupInputMonitoringPermissions()
    }
    
    // MARK: - Accessibility Permissions
    private func setupAccessibilityPermissions() {
        logger.log("🔧 Setting up Accessibility permissions...")
        
        let script = """
        tell application "System Events"
            set accessibilityEnabled to true
        end tell
        """
        
        let appleScript = NSAppleScript(source: script)
        var errorInfo: NSDictionary?
        
        if appleScript?.executeAndReturnError(&errorInfo) == nil {
            logger.log("✅ Accessibility permissions configured")
        } else {
            logger.log("⚠️ Accessibility permissions may require manual setup")
        }
    }
    
    // MARK: - Automation Permissions
    private func setupAutomationPermissions() {
        logger.log("🔧 Setting up Automation permissions...")
        
        // Create helper script for automation permissions
        let automationScript = """
        -- Request automation permissions for Calendar and Reminders
        tell application "System Events"
            -- Try to access Calendar to trigger permission request
            try
                tell application "Calendar"
                    get name
                end tell
            on error
                -- Calendar access denied, will prompt user
            end try
            
            -- Try to access Reminders to trigger permission request  
            try
                tell application "Reminders"
                    get name
                end tell
            on error
                -- Reminders access denied, will prompt user
            end try
        end tell
        """
        
        let appleScript = NSAppleScript(source: automationScript)
        var errorInfo: NSDictionary?
        
        if appleScript?.executeAndReturnError(&errorInfo) == nil {
            logger.log("✅ Automation permissions configured")
        } else {
            logger.log("⚠️ Automation permissions require user approval")
            showPermissionAlert(for: "Calendar & Reminders")
        }
    }
    
    // MARK: - Full Disk Access
    private func setupFullDiskAccess() {
        logger.log("🔧 Setting up Full Disk Access...")
        
        // Add to TCC.db for full disk access
        let script = """
        do shell script "sudo sqlite3 /Library/Application\\ Support/com.apple.TCC/TCC.db 'INSERT OR REPLACE INTO access (service,client,client_type,allowed,auth_version,auth_reason,flags,last_modified) VALUES (\"kTCCServiceSystemPolicyAllFiles\",\"/usr/local/bin/mcp-server-macos-use\",0,1,1,4,0,(strftime(\"%s\",\"now\")));'" with administrator privileges
        """
        
        let appleScript = NSAppleScript(source: script)
        var errorInfo: NSDictionary?
        
        if appleScript?.executeAndReturnError(&errorInfo) == nil {
            logger.log("✅ Full Disk Access configured")
        } else {
            logger.log("⚠️ Full Disk Access may require manual setup")
        }
    }
    
    // MARK: - Screen Recording Permissions
    private func setupScreenRecordingPermissions() {
        logger.log("🔧 Setting up Screen Recording permissions...")
        
        let script = """
        do shell script "sudo sqlite3 /Library/Application\\ Support/com.apple.TCC/TCC.db 'INSERT OR REPLACE INTO access (service,client,client_type,allowed,auth_version,auth_reason,flags,last_modified) VALUES (\"kTCCServiceScreenCapture\",\"/usr/local/bin/mcp-server-macos-use\",0,1,1,4,0,(strftime(\"%s\",\"now\")));'" with administrator privileges
        """
        
        let appleScript = NSAppleScript(source: script)
        var errorInfo: NSDictionary?
        
        if appleScript?.executeAndReturnError(&errorInfo) == nil {
            logger.log("✅ Screen Recording permissions configured")
        } else {
            logger.log("⚠️ Screen Recording may require manual setup")
        }
    }
    
    // MARK: - Input Monitoring Permissions
    private func setupInputMonitoringPermissions() {
        logger.log("🔧 Setting up Input Monitoring permissions...")
        
        let script = """
        do shell script "sudo sqlite3 /Library/Application\\ Support/com.apple.TCC/TCC.db 'INSERT OR REPLACE INTO access (service,client,client_type,allowed,auth_version,auth_reason,flags,last_modified) VALUES (\"kTCCServicePostEvent\",\"/usr/local/bin/mcp-server-macos-use\",0,1,1,4,0,(strftime(\"%s\",\"now\")));'" with administrator privileges
        """
        
        let appleScript = NSAppleScript(source: script)
        var errorInfo: NSDictionary?
        
        if appleScript?.executeAndReturnError(&errorInfo) == nil {
            logger.log("✅ Input Monitoring permissions configured")
        } else {
            logger.log("⚠️ Input Monitoring may require manual setup")
        }
    }
    
    // MARK: - Permission Alert
    private func showPermissionAlert(for service: String) {
        let alert = NSAlert()
        alert.messageText = "macOS MCP Server - Permission Required"
        alert.informativeText = """
        The server needs access to \(service) to function properly.
        
        Please grant permission in:
        System Preferences > Security & Privacy > Privacy > Automation
        
        Then check the box next to Calendar and Reminders.
        """
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Open System Preferences")
        alert.addButton(withTitle: "Later")
        
        let response = alert.runModal()
        
        if response == .alertFirstButtonReturn {
            // Open System Preferences
            let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation")!
            NSWorkspace.shared.open(url)
        }
    }
}

// MARK: - Logger
class Logger {
    func log(_ message: String) {
        let timestamp = DateFormatter().string(from: Date(), format: "yyyy-MM-dd HH:mm:ss")
        print("[\(timestamp)] \(message)")
    }
}

// MARK: - Date Formatter Extension
extension DateFormatter {
    func string(from date: Date, format: String) -> String {
        self.dateFormat = format
        return self.string(from: date)
    }
}

// MARK: - Main Execution
let setup = PermissionSetup()
setup.setupAllPermissions()
