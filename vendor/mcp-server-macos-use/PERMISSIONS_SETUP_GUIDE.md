# 🚀 macOS MCP Server - Automatic Permissions Setup Guide

## 📋 Overview

The macOS MCP Server now includes automatic permission setup that runs on first launch. This ensures all necessary permissions are configured correctly without manual intervention.

## 🔧 Automatic Setup Features

### 1. First Run Detection
- Detects first launch automatically
- Creates setup completion marker
- Only runs once per system

### 2. Automatic Permissions (No User Prompt)
- **Accessibility**: Enabled automatically
- **Full Disk Access**: Configured via TCC database
- **Screen Recording**: Configured via TCC database  
- **Input Monitoring**: Configured via TCC database

### 3. User-Requested Permissions (With Prompt)
- **Calendar & Reminders**: Requires user approval
- **Automation**: Shows user-friendly dialog
- **System Preferences**: Opens directly to correct section

## 📦 Installation Methods

### Method 1: Automatic Installation Script
```bash
cd vendor/mcp-server-macos-use
./install.sh
```

### Method 2: Manual Installation
```bash
# Build the server
swift build -c release

# Install binary
sudo cp .build/release/mcp-server-macos-use /usr/local/bin/
sudo chmod +x /usr/local/bin/mcp-server-macos-use

# Run server (will auto-setup permissions on first run)
mcp-server-macos-use
```

### Method 3: Development Setup
```bash
# Build and run directly
swift run
```

## 🔍 Permission Details

### Automatic Permissions
These are configured without user interaction:

1. **Accessibility**
   - Required for UI automation
   - Enabled via System Events
   - No user prompt needed

2. **Full Disk Access**
   - Required for file system access
   - Added to TCC database
   - Requires admin privileges (one-time)

3. **Screen Recording**
   - Required for screenshots
   - Added to TCC database
   - Requires admin privileges (one-time)

4. **Input Monitoring**
   - Required for keyboard/mouse events
   - Added to TCC database
   - Requires admin privileges (one-time)

### User-Requested Permissions
These require user approval:

1. **Calendar Access**
   - Required for calendar events
   - Shows permission dialog
   - User must approve in System Preferences

2. **Reminders Access**
   - Required for reminder management
   - Shows permission dialog
   - User must approve in System Preferences

## �� What Happens on First Run

1. **Detection**: Server checks for `.macos_mcp_setup_complete` marker
2. **Auto-Setup**: Configures automatic permissions
3. **User Prompts**: Requests Calendar/Reminders permissions
4. **System Preferences**: Opens Automation section
5. **Marker Creation**: Creates completion marker
6. **Server Start**: Begins normal operation

## 🛠️ Troubleshooting

### Permission Issues
```bash
# Reset and reinstall
rm ~/.macos_mcp_setup_complete
./install.sh
```

### Manual Permission Setup
1. Open **System Preferences**
2. Go to **Security & Privacy > Privacy**
3. Configure:
   - **Accessibility**: Check "mcp-server-macos-use"
   - **Automation**: Check "Calendar" and "Reminders"
   - **Full Disk Access**: Add "mcp-server-macos-use"
   - **Screen Recording**: Add "mcp-server-macos-use"
   - **Input Monitoring**: Add "mcp-server-macos-use"

### Check Permission Status
```bash
# Check if setup completed
ls -la ~/.macos_mcp_setup_complete

# Check server status
launchctl list | grep com.macos.mcpserver
```

## 🔧 Development Notes

### First Run Logic
```swift
private static func checkAndSetupPermissions() {
    let markerPath = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".macos_mcp_setup_complete")
    
    if !FileManager.default.fileExists(atPath: markerPath.path) {
        setupPermissions()
        createFirstRunMarker()
    }
}
```

### Permission Scripts
The server includes AppleScript handlers for:
- Automatic permission configuration
- User permission requests
- System Preferences navigation

## 🚀 Auto-Start Configuration

The server can be configured to start automatically:

```bash
# Create launch daemon
launchctl load ~/Library/LaunchAgents/com.macos.mcpserver.plist

# Check status
launchctl list | grep com.macos.mcpserver
```

## 📈 Benefits

1. **Zero Configuration**: Works out of the box
2. **User-Friendly**: Clear instructions and prompts
3. **Reliable**: Handles all permission types correctly
4. **Cross-Platform**: Works from any terminal/editor
5. **Development Ready**: Automatic setup for dev environments

## 🎉 Summary

The macOS MCP Server now provides a seamless first-run experience with automatic permission setup. Users can install and run the server without manual configuration, while still maintaining security and privacy through user approval for sensitive permissions like Calendar and Reminders.

---

**Last Updated**: February 10, 2026  
**Version**: 1.6.0  
**Status**: ✅ Production Ready
