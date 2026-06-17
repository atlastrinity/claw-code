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

// --- Helper to adjust drag start coordinates to avoid traffic lights ---
func adjustDragStartCoordinateIfNeeded(x: Double, y: Double) -> (Double, Double) {
    let systemWideElement = AXUIElementCreateSystemWide()
    var element: AXUIElement?
    let error = AXUIElementCopyElementAtPosition(systemWideElement, Float(x), Float(y), &element)
    
    if error == .success, let element = element {
        // Traverse up to find the window
        var currentElement = element
        while true {
            var role: CFTypeRef?
            if AXUIElementCopyAttributeValue(currentElement, kAXRoleAttribute as CFString, &role) == .success,
               let roleStr = role as? String, roleStr == "AXWindow" {
                
                // Found the window! Get its position and size
                var positionRef: CFTypeRef?
                var sizeRef: CFTypeRef?
                
                if AXUIElementCopyAttributeValue(currentElement, kAXPositionAttribute as CFString, &positionRef) == .success,
                   AXUIElementCopyAttributeValue(currentElement, kAXSizeAttribute as CFString, &sizeRef) == .success {
                    
                    var position = CGPoint.zero
                    var size = CGSize.zero
                    
                    AXValueGetValue(positionRef as! AXValue, .cgPoint, &position)
                    AXValueGetValue(sizeRef as! AXValue, .cgSize, &size)
                    
                    // Check if the original (x, y) is in the title bar area
                    // Top 40 pixels of the window usually contains the title bar.
                    // Clicking near the edges (< 5px) can cause resizing instead of dragging.
                    let relativeY = y - Double(position.y)
                    
                    if relativeY >= 0 && relativeY < 40 {
                        // Check if the user is explicitly targeting a tab
                        var role: CFTypeRef?
                        if AXUIElementCopyAttributeValue(element, kAXRoleAttribute as CFString, &role) == .success,
                           let roleStr = role as? String, roleStr.contains("Tab") {
                            debugLog("log: adjustDragStartCoordinate: keeping original coordinate for tab role \(roleStr)\n", stderr)
                            return (x, y)
                        }

                        // It's in the title bar zone. Adjust X to the right side of the window title bar
                        // to avoid both traffic lights (left), resize handles (edges), and tabs (center in Chrome).
                        let newX = Double(position.x) + Double(size.width) - 60.0
                        let newY = Double(position.y) + 20.0 // Center of the typical 40px title bar
                        
                        debugLog("log: adjustDragStartCoordinate: shifting drag start from (\(x), \(y)) to safe title bar area (\(newX), \(newY))\n", stderr)
                        return (newX, newY)
                    }
                }
                break
            }
            
            // Go to parent
            var parentRef: CFTypeRef?
            if AXUIElementCopyAttributeValue(currentElement, kAXParentAttribute as CFString, &parentRef) == .success,
               let parent = parentRef {
                currentElement = parent as! AXUIElement
            } else {
                break
            }
        }
    }
    
    return (x, y)
}
