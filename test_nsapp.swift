import AppKit

print("Getting shared app")
let app = NSApplication.shared
print("Getting windows")
let windows = app.windows
print("Got \(windows.count) windows")
