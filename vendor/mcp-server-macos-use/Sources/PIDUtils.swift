import Cocoa
import ApplicationServices

// --- Helper to resolve PID (handles 0 or -1 for frontmost app, and infers from coordinates) ---
func resolvePid(_ pid: Int?, x: CGFloat? = nil, y: CGFloat? = nil) -> Int {
    if let p = pid, p > 0 {
        return p
    }
    
    // Try to infer from coordinates if provided
    if let cx = x, let cy = y {
        let systemWideElement = AXUIElementCreateSystemWide()
        var element: AXUIElement?
        let error = AXUIElementCopyElementAtPosition(systemWideElement, Float(cx), Float(cy), &element)
        if error == .success, let element = element {
            var inferredPid: pid_t = 0
            let pidError = AXUIElementGetPid(element, &inferredPid)
            if pidError == .success && inferredPid > 0 {
                debugLog("log: resolvePid: inferred PID \(inferredPid) from coordinates (\(cx), \(cy))\n", stderr)
                return Int(inferredPid)
            }
        }
    }

    // Default to frontmost application
    if let frontmost = NSWorkspace.shared.frontmostApplication {
        debugLog(
            "log: resolvePid: using frontmost application '\(frontmost.localizedName ?? "unknown")' (PID: \(frontmost.processIdentifier))\n",
            stderr)
        return Int(frontmost.processIdentifier)
    }
    return 0
}
