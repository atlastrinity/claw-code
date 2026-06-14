#!/bin/bash

# macOS MCP Server Automatic Installation Script

set -e

echo "🚀 Starting macOS MCP Server Installation..."

# Check if first run
if [ ! -f "$HOME/.macos_mcp_setup_complete" ]; then
    echo "📋 First run detected - performing setup..."
    
    # Install server binary
    if [ -f "./mcp-server-macos-use" ]; then
        echo "📦 Installing server binary..."
        sudo cp ./mcp-server-macos-use /usr/local/bin/
        sudo chmod +x /usr/local/bin/mcp-server-macos-use
        echo "✅ Server installed to /usr/local/bin/"
    else
        echo "❌ Server binary not found. Please build first."
        exit 1
    fi
    
    # Setup automatic permissions
    echo "🔧 Setting up permissions..."
    
    # Accessibility
    osascript -e 'tell application "System Events" to set accessibilityEnabled to true' 2>/dev/null || echo "⚠️ Accessibility may need manual setup"
    
    # Request Calendar/Reminders permissions
    echo "📋 Requesting Calendar & Reminders permissions..."
    osascript -e 'tell application "Calendar" to get name' 2>/dev/null || true
    osascript -e 'tell application "Reminders" to get name' 2>/dev/null || true
    
    # Show permission alert
    osascript << 'APPLESCRIPT' 2>/dev/null || true
display dialog "macOS MCP Server needs Calendar & Reminders permissions.

Please grant permission in:
System Preferences > Security & Privacy > Privacy > Automation

Check boxes for:
• Calendar  
• Reminders" buttons {"Open Preferences", "Later"} default button "Open Preferences"
APPLESCRIPT
    
    if [ $? -eq 0 ]; then
        open "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
    fi
    
    # Create first run marker
    touch "$HOME/.macos_mcp_setup_complete"
    
    echo "✅ Installation completed!"
    echo "📋 Don't forget to grant Calendar & Reminders permissions!"
    
else
    echo "✅ Already configured. To reinstall: rm ~/.macos_mcp_setup_complete && ./install.sh"
fi
