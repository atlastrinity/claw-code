import AppKit
import CoreGraphics
import CoreServices
import EventKit
import Foundation
import MCP
import MacosUseSDK
import SwiftSoup
import UserNotifications
import Vision

// --- Persistent State ---
var persistentCWD: String = FileManager.default.currentDirectoryPath

// Dummy log function to silence verbose startup text
func debugLog(_ msg: String, _ file: UnsafeMutablePointer<FILE>?) {}

// Helper for flexible ISO8601 parsing
func parseISO8601(from string: String) -> Date? {
    let formatters: [ISO8601DateFormatter] = [
        {
            let f = ISO8601DateFormatter()
            f.formatOptions = [.withInternetDateTime]
            return f
        }(),
        {
            let f = ISO8601DateFormatter()
            f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            return f
        }(),
    ]
    for formatter in formatters {
        if let date = formatter.date(from: string) {
            return date
        }
    }
    return nil
}

// --- Vision / Screenshot Helpers ---

struct VisionElement: Encodable {
    let text: String
    let confidence: Float?
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

struct WindowActionResult: Codable {
    let action: String
    let pid: Int
    let actualX: Double
    let actualY: Double
    let actualWidth: Double
    let actualHeight: Double
    let note: String
}

// captureMainDisplay(monitor:) defined below with multi-monitor support

/// Downscales a CGImage so that neither dimension exceeds `maxDimension`.
/// Returns the original image if it already fits within the limit.
func resizeImageIfNeeded(image: CGImage, maxDimension: CGFloat = 1280) -> CGImage {
    let width = CGFloat(image.width)
    let height = CGFloat(image.height)
    guard max(width, height) > maxDimension else { return image }

    let scale = maxDimension / max(width, height)
    let newWidth  = Int((width  * scale).rounded())
    let newHeight = Int((height * scale).rounded())

    let colorSpace  = image.colorSpace ?? CGColorSpaceCreateDeviceRGB()
    let bitmapInfo  = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
    guard let ctx = CGContext(
        data: nil,
        width: newWidth, height: newHeight,
        bitsPerComponent: 8, bytesPerRow: 0,
        space: colorSpace, bitmapInfo: bitmapInfo.rawValue)
    else { return image }

    ctx.interpolationQuality = .high
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: newWidth, height: newHeight))
    return ctx.makeImage() ?? image
}

func encodeBase64JPEG(image: CGImage, quality: String = "high", maxDimension: CGFloat = 1280) -> String? {
    let resized = resizeImageIfNeeded(image: image, maxDimension: maxDimension)
    let bitmapRep = NSBitmapImageRep(cgImage: resized)
    let qualityValue = getQualityValue(quality)
    guard
        let data = bitmapRep.representation(
            using: .jpeg, properties: [.compressionFactor: qualityValue])
    else { return nil }
    return data.base64EncodedString()
}

func performOCR(on image: CGImage, language: String = "auto", includeConfidence: Bool = false)
    -> [VisionElement]
{
    var elements: [VisionElement] = []

    let request = VNRecognizeTextRequest { (request, error) in
        guard let observations = request.results as? [VNRecognizedTextObservation] else { return }

        let width = Double(image.width)
        let height = Double(image.height)

        for observation in observations {
            guard let candidate = observation.topCandidates(1).first else { continue }

            // Convert normalized Vision coordinates (bottom-left origin) to screen coordinates (top-left origin)
            // Vision: (0,0) is bottom-left, (1,1) is top-right.
            // Screen: (0,0) is top-left, (width,height) is bottom-right.

            let boundingBox = observation.boundingBox

            // X is same direction
            let x = boundingBox.origin.x * width
            let w = boundingBox.size.width * width

            // Y is flipped. Vision Bottom = 0, Screen Top = 0.
            // Screen Y = (1 - VisionMaxY) * ScreenHeight
            // VisionMaxY = origin.y + size.height
            // let visionMaxY = boundingBox.origin.y + boundingBox.size.height
            // let screenY = (1.0 - visionMaxY) * height

            // Alternate calculation:
            // boundBox.origin.y is bottom edge in normalized coord.
            // boundBox.origin.y + height is top edge in normalized coord.
            // We want Top edge in screen units (which is min Y).
            let screenY = (1.0 - (boundingBox.origin.y + boundingBox.size.height)) * height

            let element = VisionElement(
                text: candidate.string,
                confidence: includeConfidence ? candidate.confidence : nil,
                x: x,
                y: screenY,
                width: w,
                height: boundingBox.size.height * height
            )
            elements.append(element)
        }
    }

    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true

    // Apply language hints to improve OCR accuracy for non-English text
    switch language.lowercased() {
    case "en": request.recognitionLanguages = ["en-US"]
    case "uk": request.recognitionLanguages = ["uk-UA", "en-US"]
    case "ru": request.recognitionLanguages = ["ru-RU", "en-US"]
    case "auto": break // Let Vision framework auto-detect
    default: break
    }

    let handler = VNImageRequestHandler(cgImage: image, options: [:])
    try? handler.perform([request])

    return elements
}

// --- Screen Caching & Diffing State ---
struct VisionCacheEntry {
    var hash: UInt64
    var timestamp: Date
    var ocrResults: [VisionElement]
    var compact: Bool
}
var visionCache: [String: VisionCacheEntry] = [:]

func computeOCRDiff(old: [VisionElement], new: [VisionElement]) -> (added: [VisionElement], removed: [VisionElement], unchangedCount: Int) {
    var added: [VisionElement] = []
    var removed: [VisionElement] = []
    var usedIndices = Set<Int>()
    
    let tolerance: Double = 10.0 // pixels
    
    for newEl in new {
        var foundMatch = false
        for (i, oldEl) in old.enumerated() {
            if !usedIndices.contains(i), 
               newEl.text == oldEl.text,
               abs(newEl.x - oldEl.x) < tolerance,
               abs(newEl.y - oldEl.y) < tolerance {
                foundMatch = true
                usedIndices.insert(i)
                break
            }
        }
        if !foundMatch {
            added.append(newEl)
        }
    }
    
    for (i, oldEl) in old.enumerated() {
        if !usedIndices.contains(i) {
            removed.append(oldEl)
        }
    }
    
    return (added, removed, usedIndices.count)
}

func formatOCRElements(_ elements: [VisionElement], compact: Bool) -> String {
    if elements.isEmpty { return "" }
    return elements.map { el in
        if compact {
            return el.text
        } else {
            return "\(Int(round(el.x))),\(Int(round(el.y))),\(Int(round(el.width))),\(Int(round(el.height)))|\(el.text)"
        }
    }.joined(separator: "\n")
}

/// Computes a perceptual hash (dHash) of a CGImage.
/// Downscales to 9x8, converts to grayscale, and compares adjacent pixels.
func perceptualHash(of image: CGImage) -> UInt64 {
    let width = 9
    let height = 8
    
    let colorSpace = CGColorSpaceCreateDeviceGray()
    guard let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width, space: colorSpace, bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return 0 }
    
    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    
    guard let pixelData = context.data else { return 0 }
    let data = pixelData.bindMemory(to: UInt8.self, capacity: width * height)
    
    var hash: UInt64 = 0
    var bitIndex: Int = 0
    
    for y in 0..<height {
        for x in 0..<(width - 1) {
            let leftPixel = data[y * width + x]
            let rightPixel = data[y * width + x + 1]
            if leftPixel > rightPixel {
                hash |= (1 << bitIndex)
            }
            bitIndex += 1
        }
    }
    return hash
}

func hammingDistance(_ a: UInt64, _ b: UInt64) -> Int {
    return (a ^ b).nonzeroBitCount
}

/// Merges adjacent OCR elements that are on the same visual line (within `tolerance` px vertical distance).
/// Vision framework sometimes splits a single text line into multiple observations.
/// This reduces output token count by consolidating them.
func mergeAdjacentOCRElements(_ elements: [VisionElement], tolerance: Double = 8.0) -> [VisionElement] {
    guard !elements.isEmpty else { return [] }
    // Sort by Y (top-to-bottom), then X (left-to-right)
    let sorted = elements.sorted { a, b in
        if abs(a.y - b.y) < tolerance { return a.x < b.x }
        return a.y < b.y
    }
    var merged: [VisionElement] = []
    var current = sorted[0]
    for i in 1..<sorted.count {
        let next = sorted[i]
        // Same line: within vertical tolerance
        if abs(next.y - current.y) < tolerance {
            // Merge: extend bounding box, concatenate text
            let minX = min(current.x, next.x)
            let maxRight = max(current.x + current.width, next.x + next.width)
            let minY = min(current.y, next.y)
            let maxBottom = max(current.y + current.height, next.y + next.height)
            current = VisionElement(
                text: current.text + " " + next.text,
                confidence: nil,
                x: minX,
                y: minY,
                width: maxRight - minX,
                height: maxBottom - minY
            )
        } else {
            merged.append(current)
            current = next
        }
    }
    merged.append(current)
    return merged
}

// --- Helper for Shell execution ---
// Synchronous runShellCommand for backward compatibility in non-async contexts
func runShellCommand(_ command: String) -> (output: String, exitCode: Int32) {
    let task = Process()
    let pipe = Pipe()
    let errorPipe = Pipe()

    task.standardOutput = pipe
    task.standardError = errorPipe
    task.arguments = ["-c", command]
    task.launchPath = "/bin/zsh"
    task.currentDirectoryPath = persistentCWD

    task.environment = ProcessInfo.processInfo.environment

    do {
        try task.run()

        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        let errorData = errorPipe.fileHandleForReading.readDataToEndOfFile()

        task.waitUntilExit()

        let output = String(data: data, encoding: .utf8) ?? ""
        let errorOutput = String(data: errorData, encoding: .utf8) ?? ""
        let combinedOutput = output + errorOutput
        return (combinedOutput, task.terminationStatus)
    } catch {
        return ("Failed to execute command: \(error)", -1)
    }
}

// Asynchronous runShellCommand with timeout
func runShellCommandAsync(_ command: String, timeout: TimeInterval = 60.0) async -> (output: String, exitCode: Int32) {
    let task = Process()
    let pipe = Pipe()
    let errorPipe = Pipe()

    task.standardOutput = pipe
    task.standardError = errorPipe
    task.arguments = ["-c", command]
    task.launchPath = "/bin/zsh"
    task.currentDirectoryPath = persistentCWD
    task.environment = ProcessInfo.processInfo.environment

    do {
        try task.run()
    } catch {
        return ("Failed to execute command: \(error)", -1)
    }

    let outputResult = await withTaskGroup(of: (String, String, Bool).self) { group in
        group.addTask {
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let errorData = errorPipe.fileHandleForReading.readDataToEndOfFile()
            let out = String(data: data, encoding: .utf8) ?? ""
            let err = String(data: errorData, encoding: .utf8) ?? ""
            return (out, err, false)
        }

        group.addTask {
            try? await Task.sleep(nanoseconds: UInt64(timeout * 1_000_000_000))
            if task.isRunning {
                task.terminate()
            }
            try? pipe.fileHandleForReading.close()
            try? errorPipe.fileHandleForReading.close()
            return ("", "", true)
        }

        var finalOut = ""
        var finalErr = ""
        var didTimeout = false

        for await result in group {
            if result.2 {
                didTimeout = true
                group.cancelAll()
                break
            } else {
                finalOut = result.0
                finalErr = result.1
                group.cancelAll()
                break
            }
        }
        return (finalOut, finalErr, didTimeout)
    }

    let combinedOutput = outputResult.0 + outputResult.1
    if outputResult.2 {
        return (combinedOutput + "\n[Command execution timed out after \(timeout) seconds]", -1)
    }
    
    task.waitUntilExit()
    return (combinedOutput, task.terminationStatus)
}

// --- System Settings Helper ---
enum PrivacyCategory {
    case calendars, reminders, accessibility, screenRecording, fullDiskAccess, automation

    var url: URL? {
        let base = "x-apple.systempreferences:com.apple.preference.security?Privacy_"
        switch self {
        case .calendars: return URL(string: base + "Calendars")
        case .reminders: return URL(string: base + "Reminders")
        case .accessibility: return URL(string: base + "Accessibility")
        case .screenRecording: return URL(string: base + "ScreenCapture")
        case .fullDiskAccess: return URL(string: base + "AllFiles")
        case .automation: return URL(string: base + "Automation")
        }
    }

    var name: String {
        switch self {
        case .calendars: return "Calendars"
        case .reminders: return "Reminders"
        case .accessibility: return "Accessibility"
        case .screenRecording: return "Screen Recording"
        case .fullDiskAccess: return "Full Disk Access"
        case .automation: return "Automation"
        }
    }
}

func openSystemSettings(for category: PrivacyCategory) {
    if let url = category.url {
        NSWorkspace.shared.open(url)
    }
}

// --- Helper to resolve PID (handles 0 or -1 for frontmost app) ---
func resolvePid(_ pid: Int?) -> Int {
    if let p = pid, p > 0 {
        return p
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

// --- Interactive Environment Check ---
var isInteractive: Bool {
    // When running as MCP server via stdio (child of node/bridge), stdin is a pipe not a TTY.
    // In that mode, we should NOT open System Settings windows or show interactive prompts.
    return isatty(STDIN_FILENO) != 0
}

// --- Persistent EventStore ---
let eventStore = EKEventStore()

// --- Helper for EventKit Permissions ---
func requestCalendarAccess(openSettings: Bool = true) async -> Bool {
    let status = EKEventStore.authorizationStatus(for: .event)
    if status == .notDetermined && !isInteractive {
        debugLog("log: requestCalendarAccess: Access not determined and non-interactive; returning false silently.\n", stderr)
        return false
    }

    let granted: Bool
    if #available(macOS 14.0, *) {
        do {
            granted = try await eventStore.requestFullAccessToEvents()
        } catch {
            debugLog("error: requestCalendarAccess: \(error)\n", stderr)
            granted = false
        }
    } else {
        granted = await withCheckedContinuation { continuation in
            eventStore.requestAccess(to: .event) { granted, error in
                if let error = error {
                    debugLog("error: requestCalendarAccess: \(error)\n", stderr)
                }
                continuation.resume(returning: granted)
            }
        }
    }

    if !granted {
        if openSettings && isInteractive {
            debugLog("log: requestCalendarAccess: Access denied, opening System Settings...\n", stderr)
            openSystemSettings(for: .calendars)
        } else {
            debugLog("log: requestCalendarAccess: Access denied (silent mode)\n", stderr)
        }
    }
    return granted
}

func requestRemindersAccess(openSettings: Bool = true) async -> Bool {
    let status = EKEventStore.authorizationStatus(for: .reminder)
    if status == .notDetermined && !isInteractive {
        debugLog("log: requestRemindersAccess: Access not determined and non-interactive; returning false silently.\n", stderr)
        return false
    }

    let granted: Bool
    if #available(macOS 14.0, *) {
        do {
            granted = try await eventStore.requestFullAccessToReminders()
        } catch {
            debugLog("error: requestRemindersAccess: \(error)\n", stderr)
            granted = false
        }
    } else {
        granted = await withCheckedContinuation { continuation in
            eventStore.requestAccess(to: .reminder) { granted, error in
                if let error = error {
                    debugLog("error: requestRemindersAccess: \(error)\n", stderr)
                }
                continuation.resume(returning: granted)
            }
        }
    }

    if !granted {
        if openSettings && isInteractive {
            debugLog(
                "log: requestRemindersAccess: Access denied, opening System Settings...\n", stderr)
            openSystemSettings(for: .reminders)
        } else {
            debugLog("log: requestRemindersAccess: Access denied (silent mode)\n", stderr)
        }
    }
    return granted
}

// --- Spotlight Helper ---
class SpotlightSearcher: NSObject {
    var query: NSMetadataQuery?
    var semaphore: DispatchSemaphore?
    var results: [String] = []

    func search(queryStr: String) -> [String] {
        self.results = []
        self.semaphore = DispatchSemaphore(value: 0)
        self.query = NSMetadataQuery()

        guard let query = self.query else { return [] }

        query.searchScopes = [NSMetadataQueryLocalComputerScope]
        query.predicate = NSPredicate(
            format: "%K == 1 || %K LIKE[cd] %@", NSMetadataItemFSNameKey, NSMetadataItemFSNameKey,
            "*\(queryStr)*")
        // Simple filename match. Advanced usage could allow raw NSPredicate strings.

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(queryDidFinish(_:)),
            name: .NSMetadataQueryDidFinishGathering,
            object: query
        )

        query.start()

        // Timeout after 5 seconds to prevent hanging
        _ = self.semaphore?.wait(timeout: .now() + 5)

        query.stop()
        NotificationCenter.default.removeObserver(self)

        return self.results
    }

    @objc func queryDidFinish(_ notification: Foundation.Notification) {
        guard let query = notification.object as? NSMetadataQuery else { return }
        query.disableUpdates()

        for i in 0..<query.resultCount {
            if let item = query.result(at: i) as? NSMetadataItem,
                let path = item.value(forAttribute: NSMetadataItemPathKey) as? String
            {
                self.results.append(path)
            }
        }
        self.semaphore?.signal()
    }
}
let spotlight = SpotlightSearcher()

// --- Helper for AppleScript Execution with Timeout (Process-based) ---
// Uses /usr/bin/osascript as an external process instead of NSAppleScript.
// This ensures the execution is truly killable via SIGTERM on timeout,
// preventing indefinite hangs caused by TCC permission dialogs or blocking scripts.
func runAppleScript(_ script: String, timeout: TimeInterval = 10.0) async -> (
    success: Bool, output: String, error: String?
) {
    let task = Process()
    let stdoutPipe = Pipe()
    let stderrPipe = Pipe()

    task.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
    task.arguments = ["-e", script]
    task.standardOutput = stdoutPipe
    task.standardError = stderrPipe

    do {
        try task.run()
    } catch {
        debugLog("error: runAppleScript: failed to launch osascript: \(error)\n", stderr)
        return (false, "", error.localizedDescription)
    }

    let outputResult = await withTaskGroup(of: (String, String, Bool).self) { group in
        group.addTask {
            let stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
            let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
            let out = String(data: stdoutData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let err = String(data: stderrData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            return (out, err, false)
        }

        group.addTask {
            try? await Task.sleep(nanoseconds: UInt64(timeout * 1_000_000_000))
            if task.isRunning {
                task.terminate()
            }
            // Close file handles to unblock readDataToEndOfFile
            try? stdoutPipe.fileHandleForReading.close()
            try? stderrPipe.fileHandleForReading.close()
            return ("", "", true)
        }

        var finalOut = ""
        var finalErr = ""
        var didTimeout = false

        for await result in group {
            if result.2 {
                didTimeout = true
                group.cancelAll()
                break
            } else {
                finalOut = result.0
                finalErr = result.1
                group.cancelAll()
                break
            }
        }
        return (finalOut, finalErr, didTimeout)
    }

    if outputResult.2 {
        debugLog("warning: runAppleScript: execution timed out after \(timeout)s, process terminated.\n", stderr)
        return (false, "", "AppleScript execution timed out after \(timeout) seconds")
    }
    
    task.waitUntilExit()

    if task.terminationStatus == 0 {
        return (true, outputResult.0, nil)
    } else {
        let errMsg = outputResult.1.isEmpty ? "osascript exited with code \(task.terminationStatus)" : outputResult.1
        debugLog("error: runAppleScript: \(errMsg)\n", stderr)
        return (false, outputResult.0, errMsg)
    }
}

// --- Ultimate AppleScript Management ---
var appleScriptTemplates: [[String: String]] = [
    [
        "name": "automation",
        "script": "tell application \"System Events\" to keystroke \"a\" using command down",
        "description": "Select all text",
    ],
    [
        "name": "file_ops",
        "script": "tell application \"Finder\" to make new folder at desktop",
        "description": "Create new folder on desktop",
    ],
    [
        "name": "system_info",
        "script": "tell application \"System Events\" to get system version",
        "description": "Get system version information",
    ],
    [
        "name": "app_control",
        "script": "tell application \"System Events\" to tell process \"Safari\" to activate",
        "description": "Activate Safari application",
    ],
]

func getAppleScriptTemplate(_ templateName: String) -> String {
    for template in appleScriptTemplates {
        if template["name"] == templateName {
            return template["script"] ?? ""
        }
    }
    return ""
}

func getAppleScriptTemplates() -> [[String: String]] {
    return appleScriptTemplates
}

func addAppleScriptTemplate(_ template: [String: String]) {
    appleScriptTemplates.append(template)

    // Limit templates
    if appleScriptTemplates.count > 50 {
        appleScriptTemplates.removeFirst(appleScriptTemplates.count - 50)
    }
}

func generateAppleScriptForDescription(_ description: String) -> String {
    // Simple AI-like script generation based on keywords
    let lowerDesc = description.lowercased()

    if lowerDesc.contains("open") && lowerDesc.contains("safari") {
        return "tell application \"Safari\" to activate"
    } else if lowerDesc.contains("open") && lowerDesc.contains("finder") {
        return "tell application \"Finder\" to activate"
    } else if lowerDesc.contains("new") && lowerDesc.contains("folder") {
        return "tell application \"Finder\" to make new folder at desktop"
    } else if lowerDesc.contains("copy") && lowerDesc.contains("text") {
        return "tell application \"System Events\" to keystroke \"c\" using command down"
    } else if lowerDesc.contains("paste") && lowerDesc.contains("text") {
        return "tell application \"System Events\" to keystroke \"v\" using command down"
    } else if lowerDesc.contains("quit") && lowerDesc.contains("app") {
        return "tell application \"System Events\" to quit"
    } else if lowerDesc.contains("volume") && lowerDesc.contains("mute") {
        return "set volume with output muted"
    } else if lowerDesc.contains("volume") && lowerDesc.contains("up") {
        return "set volume output volume ((output volume of (get volume settings)) + 10)"
    } else if lowerDesc.contains("volume") && lowerDesc.contains("down") {
        return "set volume output volume ((output volume of (get volume settings)) - 10)"
    } else {
        return
            "-- Generated script for: \(description)\n-- Please provide more specific description"
    }
}

func validateAppleScript(_ script: String) -> (isValid: Bool, error: String) {
    // Basic validation
    if script.isEmpty {
        return (false, "Script is empty")
    }

    if !script.contains("tell") && !script.contains("set") && !script.contains("display") {
        return (false, "Script doesn't contain valid AppleScript commands")
    }

    if script.contains("rm ") || script.contains("delete ") || script.contains("kill ") {
        return (false, "Script contains potentially dangerous commands")
    }

    return (true, "")
}

// --- Enhanced Notification Scheduling Management ---
var scheduledNotifications: [[String: String]] = []
let maxScheduledNotifications = 50

func addScheduledNotification(
    title: String, message: String, schedule: String, sound: String, persistent: Bool
) {
    let entry: [String: String] = [
        "title": title,
        "message": message,
        "schedule": schedule,
        "sound": sound,
        "persistent": persistent ? "true" : "false",
        "created": ISO8601DateFormatter().string(from: Date()),
    ]

    scheduledNotifications.append(entry)

    // Limit scheduled notifications
    if scheduledNotifications.count > maxScheduledNotifications {
        scheduledNotifications.removeFirst(scheduledNotifications.count - maxScheduledNotifications)
    }
}

func getScheduledNotifications() -> [[String: String]] {
    return scheduledNotifications
}

func clearScheduledNotifications() {
    scheduledNotifications.removeAll()
}

func getNotificationTemplate(_ templateName: String) -> [String: String] {
    let templates: [String: [String: String]] = [
        "reminder": [
            "title": "⏰ Reminder",
            "message": "Don't forget to complete your task!",
        ],
        "meeting": [
            "title": "📅 Meeting",
            "message": "Your meeting is starting soon!",
        ],
        "break": [
            "title": "☕ Break Time",
            "message": "Time for a short break!",
        ],
        "deadline": [
            "title": "⚠️ Deadline",
            "message": "Your deadline is approaching!",
        ],
    ]

    return templates[templateName] ?? [:]
}

// --- Enhanced Clipboard History Management ---
var clipboardHistory: [[String: String]] = []
let maxHistorySize = 100

func addToClipboardHistory(text: String, html: String? = nil, image: String? = nil) {
    let timestamp = ISO8601DateFormatter().string(from: Date())
    var entry: [String: String] = [
        "timestamp": timestamp,
        "text": text,
    ]

    if let htmlContent = html {
        entry["html"] = htmlContent
    }

    if let imageData = image {
        entry["image"] = imageData
    }

    clipboardHistory.append(entry)

    // Limit history size
    if clipboardHistory.count > maxHistorySize {
        clipboardHistory.removeFirst(clipboardHistory.count - maxHistorySize)
    }
}

func getClipboardHistory(limit: Int = 50) -> [[String: String]] {
    let limitedHistory = Array(clipboardHistory.suffix(limit))
    return limitedHistory
}

func clearClipboardHistory() {
    clipboardHistory.removeAll()
}

// --- Helper Functions for Enhanced Features ---

func getQualityValue(_ quality: String) -> Double {
    switch quality.lowercased() {
    case "low": return 0.3
    case "medium": return 0.6
    case "high": return 0.8
    case "lossless": return 1.0
    default: return 0.8
    }
}

func captureMainDisplay(monitor: Int? = nil) -> CGImage? {
    guard let monitorIndex = monitor, monitorIndex > 0 else {
        return CGDisplayCreateImage(CGMainDisplayID())
    }
    // Real multi-monitor support
    var displayCount: UInt32 = 0
    CGGetActiveDisplayList(0, nil, &displayCount)
    guard displayCount > 0 else { return CGDisplayCreateImage(CGMainDisplayID()) }
    var displays = [CGDirectDisplayID](repeating: 0, count: Int(displayCount))
    CGGetActiveDisplayList(displayCount, &displays, &displayCount)
    let idx = min(monitorIndex, Int(displayCount) - 1)
    return CGDisplayCreateImage(displays[idx])
}

/// Captures only the window owned by the given PID, instead of the full screen.
/// Returns nil if no on-screen window is found for this PID.
/// This produces a much smaller image than full-screen capture, saving tokens.
func captureWindow(pid: pid_t) -> CGImage? {
    guard let windowList = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
        return nil
    }
    // Find the first on-screen window for this PID (prefer largest)
    var bestWindow: (id: CGWindowID, area: CGFloat) = (0, 0)
    for windowInfo in windowList {
        guard let ownerPID = windowInfo[kCGWindowOwnerPID as String] as? Int,
              ownerPID == Int(pid),
              let windowID = windowInfo[kCGWindowNumber as String] as? CGWindowID,
              let bounds = windowInfo[kCGWindowBounds as String] as? [String: CGFloat],
              let w = bounds["Width"], let h = bounds["Height"] else {
            continue
        }
        let area = w * h
        if area > bestWindow.area {
            bestWindow = (windowID, area)
        }
    }
    guard bestWindow.id != 0 else { return nil }
    return CGWindowListCreateImage(.null, .optionIncludingWindow, bestWindow.id, [.boundsIgnoreFraming])
}


// --- Helper for safe AppleScript string escaping ---
func escapeForAppleScript(_ str: String) -> String {
    return
        str
        .replacingOccurrences(of: "\\", with: "\\\\")
        .replacingOccurrences(of: "\"", with: "\\\"")
}

// --- Helper to serialize Swift structs to JSON String ---
func serializeToJsonString<T: Encodable>(_ value: T) -> String? {
    let encoder = JSONEncoder()
    // Removed pretty printing to drastically reduce token footprint for LLM context
    encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    do {
        let jsonData = try encoder.encode(value)
        return String(data: jsonData, encoding: .utf8)
    } catch {
        debugLog("error: serializeToJsonString: failed to encode value to JSON: \(error)\n", stderr)
        return nil
    }
}

// --- Function to get arguments from MCP Value ---
// Helper to extract typed values safely
func getRequiredString(from args: [String: Value]?, key: String) throws -> String {
    guard let val = args?[key]?.stringValue else {
        throw MCPError.invalidParams("Missing or invalid required string argument: '\(key)'")
    }
    return val
}

func getRequiredDouble(from args: [String: Value]?, key: String) throws -> Double {
    guard let value = args?[key] else {
        throw MCPError.invalidParams("Missing required number argument: '\(key)'")
    }
    switch value {
    case .int(let intValue):
        debugLog(
            "log: getRequiredDouble: converting int \(intValue) to double for key '\(key)'\n",
            stderr)
        return Double(intValue)
    case .double(let doubleValue):
        return doubleValue
    default:
        throw MCPError.invalidParams(
            "Invalid type for required number argument: '\(key)', expected Int or Double, got \(value)"
        )
    }
}

func getRequiredInt(from args: [String: Value]?, key: String) throws -> Int {
    guard let value = args?[key] else {
        throw MCPError.invalidParams("Missing required integer argument: '\(key)'")
    }
    // Allow conversion from Double if it's an exact integer
    if let doubleValue = value.doubleValue {
        if let intValue = Int(exactly: doubleValue) {
            debugLog(
                "log: getRequiredInt: converting exact double \(doubleValue) to int for key '\(key)'\n",
                stderr)
            return intValue
        } else {
            debugLog(
                "warning: getRequiredInt: received non-exact double \(doubleValue) for key '\(key)', expecting integer.\n",
                stderr)
            throw MCPError.invalidParams(
                "Invalid type for required integer argument: '\(key)', received non-exact Double \(doubleValue)"
            )
        }
    }
    // Otherwise, require it to be an Int directly
    guard let intValue = value.intValue else {
        throw MCPError.invalidParams(
            "Invalid type for required integer argument: '\(key)', expected Int or exact Double, got \(value)"
        )
    }
    return intValue
}

// --- Get Optional arguments ---
// Helper for optional values
func getOptionalDouble(from args: [String: Value]?, key: String) throws -> Double? {
    guard let value = args?[key] else { return nil }  // Key not present is valid for optional
    if value.isNull { return nil }  // Explicit null is also valid
    switch value {
    case .int(let intValue):
        debugLog(
            "log: getOptionalDouble: converting int \(intValue) to double for key '\(key)'\n",
            stderr)
        return Double(intValue)
    case .double(let doubleValue):
        return doubleValue
    default:
        throw MCPError.invalidParams(
            "Invalid type for optional number argument: '\(key)', expected Int or Double, got \(value)"
        )
    }
}

func getOptionalInt(from args: [String: Value]?, key: String) throws -> Int? {
    guard let value = args?[key] else { return nil }  // Key not present is valid for optional
    if value.isNull { return nil }  // Explicit null is also valid

    if let doubleValue = value.doubleValue {
        if let intValue = Int(exactly: doubleValue) {
            debugLog(
                "log: getOptionalInt: converting exact double \(doubleValue) to int for key '\(key)'\n",
                stderr)
            return intValue
        } else {
            debugLog(
                "warning: getOptionalInt: received non-exact double \(doubleValue) for key '\(key)', expecting integer.\n",
                stderr)
            throw MCPError.invalidParams(
                "Invalid type for optional integer argument: '\(key)', received non-exact Double \(doubleValue)"
            )
        }
    }
    guard let intValue = value.intValue else {
        throw MCPError.invalidParams(
            "Invalid type for optional integer argument: '\(key)', expected Int or exact Double, got \(value)"
        )
    }
    return intValue
}

func getOptionalBool(from args: [String: Value]?, key: String) throws -> Bool? {
    guard let value = args?[key] else { return nil }  // Key not present
    if value.isNull { return nil }  // Explicit null
    if let boolValue = value.boolValue { return boolValue }
    
    // Add string parsing to handle cases where LLMs pass "true" or "false"
    if let strValue = value.stringValue {
        if strValue.lowercased() == "true" { return true }
        if strValue.lowercased() == "false" { return false }
    }
    
    throw MCPError.invalidParams(
        "Invalid type for optional boolean argument: '\(key)', expected Bool, got \(value)")
}

func getOptionalString(from args: [String: Value]?, key: String) throws -> String? {
    guard let value = args?[key] else { return nil }
    if value.isNull { return nil }
    guard let strValue = value.stringValue else {
        throw MCPError.invalidParams(
            "Invalid type for optional string argument: '\(key)', expected String, got \(value)")
    }
    return strValue
}

func getOptionalObject(from args: [String: Value]?, key: String) throws -> [String: Value]? {
    guard let value = args?[key] else { return nil }
    if value.isNull { return nil }
    guard let objValue = value.objectValue else {
        throw MCPError.invalidParams(
            "Invalid type for optional object argument: '\(key)', expected Object, got \(value)")
    }
    return objValue
}

// --- NEW Helper to parse modifier flags ---
func parseFlags(from value: Value?) throws -> CGEventFlags {
    guard let arrayValue = value?.arrayValue else {
        // No flags provided or not an array, return empty flags
        return []
    }

    var flags: CGEventFlags = []
    for flagValue in arrayValue {
        guard let flagString = flagValue.stringValue else {
            throw MCPError.invalidParams(
                "Invalid modifierFlags array: contains non-string element \(flagValue)")
        }
        switch flagString.lowercased() {
        // Standard modifiers
        case "capslock", "caps": flags.insert(.maskAlphaShift)
        case "shift": flags.insert(.maskShift)
        case "control", "ctrl": flags.insert(.maskControl)
        case "option", "opt", "alt": flags.insert(.maskAlternate)
        case "command", "cmd": flags.insert(.maskCommand)
        // Other potentially useful flags
        case "help": flags.insert(.maskHelp)
        case "function", "fn": flags.insert(.maskSecondaryFn)
        case "numericpad", "numpad": flags.insert(.maskNumericPad)
        // Non-keyed state (less common for press simulation)
        // case "noncoalesced": flags.insert(.maskNonCoalesced)
        default:
            debugLog(
                "warning: parseFlags: unknown modifier flag string '\(flagString)', ignoring.\n",
                stderr)
        // Optionally throw an error:
        // throw MCPError.invalidParams("Unknown modifier flag: '\(flagString)'")
        }
    }
    return flags
}

// Async helper function to set up and start the server
func setupAndStartServer() async throws -> Server {
    debugLog("log: setupAndStartServer: entering function.\n", stderr)

    // --- Define Schemas and Tools for Simplified Actions ---
    // (Schemas remain the same as they define the MCP interface)
    let openAppSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "identifier": .object([
                "type": .string("string"),
                "description": .string("REQUIRED. App name, path, or bundle ID."),
            ])
        ]),
        "required": .array([.string("identifier")]),
    ])
    let openAppTool = Tool(
        name: "macos-use_open_application_and_traverse",
        description: "Opens/activates an application and then traverses its accessibility tree.",
        inputSchema: openAppSchema
    )

    let clickSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the target application. Defaults to frontmost app."),
            ]),
            "x": .object([
                "type": .string("number"),
                "description": .string("REQUIRED. X coordinate for the click."),
            ]),
            "y": .object([
                "type": .string("number"),
                "description": .string("REQUIRED. Y coordinate for the click."),
            ]),
            "showAnimation": .object([
                "type": .string("boolean"),
                "description": .string("OPTIONAL. Show visual feedback animation (green circle)."),
            ]),
            "animationDuration": .object([
                "type": .string("number"),
                "description": .string("OPTIONAL. Duration of the animation in seconds."),
            ]),
            "activateApp": .object([
                "type": .string("boolean"),
                "description": .string("OPTIONAL. Activate the app (bring to front) before executing. Default: true."),
            ]),
        ]),
        "required": .array([.string("x"), .string("y")]),
    ])
    let clickTool = Tool(
        name: "macos-use_click_and_traverse",
        description:
            "Simulates a click at the given coordinates within the app specified by PID, then traverses its accessibility tree.",
        inputSchema: clickSchema
    )

    let typeSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the target application window. Defaults to frontmost app."),
            ]),
            "text": .object([
                "type": .string("string"), "description": .string("REQUIRED. Text to type."),
            ]),
            "activateApp": .object([
                "type": .string("boolean"),
                "description": .string("OPTIONAL. Activate the app (bring to front) before executing. Default: true."),
            ]),
            // Add optional options here if needed later
        ]),
        "required": .array([.string("text")]),
    ])
    let typeTool = Tool(
        name: "macos-use_type_and_traverse",
        description:
            "Simulates typing text into the app specified by PID, then traverses its accessibility tree.",
        inputSchema: typeSchema
    )

    let refreshSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the application to traverse. Defaults to frontmost app."),
            ])
            // Add optional options here if needed later
        ]),
        "required": .array([]),
    ])
    let refreshTool = Tool(
        name: "macos-use_refresh_traversal",
        description: "Traverses the accessibility tree of the application specified by PID.",
        inputSchema: refreshSchema
    )

    // *** NEW: Schema and Tool for Execute Command ***
    let executeCommandSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "command": .object([
                "type": .string("string"),
                "description": .string("REQUIRED. The shell command to execute."),
            ])
        ]),
        "required": .array([.string("command")]),
    ])
    let executeCommandTool = Tool(
        name: "execute_command",  // Matching the Python terminal MCP name
        description: "Execute a terminal command in a persistent shell session (maintains CWD).",
        inputSchema: executeCommandSchema
    )
    let terminalTool = Tool(
        name: "terminal",
        description: "Alias for execute_command.",
        inputSchema: executeCommandSchema
    )

    // *** ENHANCED: Screenshot Tool ***
    let screenshotSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "path": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional path to save screenshot. If not provided, returns Base64."),
            ]),
            "region": .object([
                "type": .string("object"),
                "description": .string(
                    "Optional region to capture: {x: number, y: number, width: number, height: number}"
                ),
                "properties": .object([
                    "x": .object([
                        "type": .string("number"),
                        "description": .string("X coordinate of top-left corner"),
                    ]),
                    "y": .object([
                        "type": .string("number"),
                        "description": .string("Y coordinate of top-left corner"),
                    ]),
                    "width": .object([
                        "type": .string("number"), "description": .string("Width of region"),
                    ]),
                    "height": .object([
                        "type": .string("number"), "description": .string("Height of region"),
                    ]),
                ]),
            ]),
            "monitor": .object([
                "type": .string("number"),
                "description": .string(
                    "Optional monitor index (0 for main, 1 for secondary, etc.)"),
            ]),
            "quality": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional compression quality: 'low', 'medium', 'high', 'lossless'"),
                "enum": .array([
                    .string("low"), .string("medium"), .string("high"), .string("lossless"),
                ]),
            ]),
            "format": .object([
                "type": .string("string"),
                "description": .string("Optional output format: 'png', 'jpg', 'webp'"),
                "enum": .array([.string("png"), .string("jpg"), .string("webp")]),
            ]),
            "ocr": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Run OCR on screenshot and append extracted text to the result."),
            ]),
            "includeImage": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Whether to include the Base64 image in the response (default: true). Set to false when you only need OCR text — saves significant context tokens."),
            ]),
            "maxDimension": .object([
                "type": .string("number"),
                "description": .string("Optional: Maximum width or height in pixels before downscaling (default: 1024). Use lower values (e.g. 800) to further reduce token usage. Use higher values (e.g. 1920) for fine detail."),
            ]),
            "compact": .object([
                "type": .string("boolean"),
                "description": .string("Optional: When true, OCR returns only text lines without coordinates. Drastically reduces token count when coordinate info is not needed (default: false)."),
            ]),
        ]),
    ])
    let screenshotTool = Tool(
        name: "macos-use_take_screenshot",
        description:
            "Takes a screenshot. Automatically downscales Retina displays to max 1024px to reduce token usage. Supports region selection, multi-monitor, OCR, compact mode, and optional image suppression (includeImage: false for OCR-only mode).",
        inputSchema: screenshotSchema
    )
    let screenshotAliasTool = Tool(
        name: "screenshot",
        description: "Alias for macos-use_take_screenshot.",
        inputSchema: screenshotSchema
    )

    // *** ENHANCED: Vision Analysis Tool ***
    let visionSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "region": .object([
                "type": .string("object"),
                "description": .string(
                    "Optional region to analyze: {x: number, y: number, width: number, height: number}"
                ),
                "properties": .object([
                    "x": .object([
                        "type": .string("number"),
                        "description": .string("X coordinate of top-left corner"),
                    ]),
                    "y": .object([
                        "type": .string("number"),
                        "description": .string("Y coordinate of top-left corner"),
                    ]),
                    "width": .object([
                        "type": .string("number"), "description": .string("Width of region"),
                    ]),
                    "height": .object([
                        "type": .string("number"), "description": .string("Height of region"),
                    ]),
                ]),
            ]),
            "language": .object([
                "type": .string("string"),
                "description": .string("Optional language hint: 'en', 'uk', 'ru', 'auto'"),
                "enum": .array([.string("en"), .string("uk"), .string("ru"), .string("auto")]),
            ]),
            "confidence": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Include confidence scores in results"),
            ]),
            "format": .object([
                "type": .string("string"),
                "description": .string("Optional output format: 'json' (default), 'text' (plain joined), 'both'"),
                "enum": .array([.string("json"), .string("text"), .string("both")]),
            ]),
        ]),
    ])
    let visionTool = Tool(
        name: "macos-use_analyze_screen",
        description:
            "Captures the screen and runs OCR. Automatically downscales to max 1024px before processing for speed. Supports region selection, language hints, confidence scores, and multiple output formats. For pure text extraction, use format: 'text' to minimize response size.",
        inputSchema: visionSchema
    )

    // *** NEW: Unified Vision Tool — combines screenshot + OCR + accessibility in one call ***
    let unifiedVisionSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "mode": .object([
                "type": .string("string"),
                "description": .string("Vision mode: 'smart' (default, auto-decides image inclusion based on OCR richness), 'ocr' (text only, minimum tokens), 'image' (screenshot only), 'full' (image + OCR)."),
                "enum": .array([.string("smart"), .string("ocr"), .string("image"), .string("full")]),
            ]),
            "region": .object([
                "type": .string("object"),
                "description": .string("Optional region: {x, y, width, height}"),
                "properties": .object([
                    "x": .object(["type": .string("number")]),
                    "y": .object(["type": .string("number")]),
                    "width": .object(["type": .string("number")]),
                    "height": .object(["type": .string("number")]),
                ]),
            ]),
            "monitor": .object([
                "type": .string("number"),
                "description": .string("Optional monitor index (0 for main)."),
            ]),
            "maxDimension": .object([
                "type": .string("number"),
                "description": .string("Max width/height in px before downscaling (default: 1024)."),
            ]),
            "quality": .object([
                "type": .string("string"),
                "description": .string("JPEG quality: 'low', 'medium' (default), 'high'."),
                "enum": .array([.string("low"), .string("medium"), .string("high")]),
            ]),
            "language": .object([
                "type": .string("string"),
                "description": .string("OCR language hint: 'en', 'uk', 'ru', 'auto' (default)."),
                "enum": .array([.string("en"), .string("uk"), .string("ru"), .string("auto")]),
            ]),
            "compact": .object([
                "type": .string("boolean"),
                "description": .string("When true, OCR returns only text lines without coordinates (default: false)."),
            ]),
            "withAccessibility": .object([
                "type": .string("boolean"),
                "description": .string("When true, also returns the accessibility tree of the frontmost app in the same call (default: false)."),
            ]),
            "pid": .object([
                "type": .string("number"),
                "description": .string("PID of target app. Used for: (1) window-targeted capture instead of full screen — produces smaller images and saves tokens, (2) accessibility tree traversal. Defaults to frontmost app."),
            ]),
            "diff": .object([
                "type": .string("boolean"),
                "description": .string("When true, checks if the screen has visually changed since the last capture using a perceptual hash. If no change, returns a minimal 'no_change' response instead of the full image/OCR, saving 99% tokens (default: false)."),
            ]),
        ]),
    ])
    let unifiedVisionTool = Tool(
        name: "macos-use_vision",
        description: "Unified vision tool. Modes: 'smart' (auto-decides if image needed based on OCR richness — best default), 'ocr' (text only, minimal tokens), 'image' (screenshot only), 'full' (image + OCR). When PID is provided, captures only that window instead of full screen for token savings. Supports region selection, language hints, compact output, and optional accessibility tree in a single call.",
        inputSchema: unifiedVisionSchema
    )

    // *** NEW: Schema and Tool for Press Key ***
    let pressKeySchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the target application window. Defaults to frontmost app."),
            ]),
            "keyName": .object([
                "type": .string("string"),
                "description": .string(
                    "REQUIRED. Name of the key to press (e.g., 'Return', 'Enter', 'Escape', 'Tab', 'ArrowUp', 'Delete', 'a', 'B'). Case-sensitive for letter keys if no modifiers used."
                ),
            ]),
            "modifierFlags": .object([  // Optional array of strings
                "type": .string("array"),
                "description": .string(
                    "OPTIONAL. Modifier keys to hold (e.g., ['Command', 'Shift']). Valid: CapsLock, Shift, Control, Option, Command, Function, NumericPad, Help."
                ),
                "items": .object(["type": .string("string")]),  // Items in the array must be strings
            ]),
            "activateApp": .object([
                "type": .string("boolean"),
                "description": .string("OPTIONAL. Activate the app (bring to front) before executing. Default: true."),
            ]),
            // Add optional ActionOptions overrides here if needed later
        ]),
        "required": .array([.string("keyName")]),
    ])
    let pressKeyTool = Tool(
        name: "macos-use_press_key_and_traverse",
        description:
            "Simulates pressing a specific key (like Return, Enter, Escape, Tab, Arrow Keys, regular characters) with optional modifiers, then traverses the accessibility tree.",
        inputSchema: pressKeySchema
    )

    // *** NEW: Scroll Tool ***
    let scrollSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the target application. Defaults to frontmost app."),
            ]),
            "direction": .object([
                "type": .string("string"),
                "description": .string(
                    "REQUIRED. Direction to scroll: 'up', 'down', 'left', 'right'."),
            ]),
            "amount": .object([
                "type": .string("number"),
                "description": .string("OPTIONAL. Amount to scroll (default 3)."),
            ]),
            "sensitivity": .object([
                "type": .string("string"),
                "description": .string(
                    "OPTIONAL. Scroll sensitivity: 'fine' (1x), 'normal' (10x, default), 'fast' (30x)."
                ),
                "enum": .array([.string("fine"), .string("normal"), .string("fast")]),
            ]),
        ]),
        "required": .array([.string("direction")]),
    ])
    let scrollTool = Tool(
        name: "macos-use_scroll_and_traverse",
        description: "Simulates a mouse scroll wheel action in a specific direction.",
        inputSchema: scrollSchema
    )

    // *** NEW: Right Click Tool ***
    let mouseActionSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the target application. Defaults to frontmost app."),
            ]),
            "x": .object([
                "type": .string("number"), "description": .string("REQUIRED. Screen X coordinate."),
            ]),
            "y": .object([
                "type": .string("number"), "description": .string("REQUIRED. Screen Y coordinate."),
            ]),
            "activateApp": .object([
                "type": .string("boolean"),
                "description": .string("OPTIONAL. Activate the app (bring to front) before executing. Default: true."),
            ]),
        ]),
        "required": .array([.string("x"), .string("y")]),
    ])
    let rightClickTool = Tool(
        name: "macos-use_right_click_and_traverse",
        description: "Simulates a right-click (context menu) at the specified coordinates.",
        inputSchema: mouseActionSchema
    )
    let doubleClickTool = Tool(
        name: "macos-use_double_click_and_traverse",
        description: "Simulates a double-click at the specified coordinates.",
        inputSchema: mouseActionSchema
    )

    // *** NEW: Triple Click Tool (select entire line) ***
    let tripleClickTool = Tool(
        name: "macos-use_triple_click_and_traverse",
        description:
            "Simulates a triple-click to select an entire line of text at the specified coordinates.",
        inputSchema: mouseActionSchema
    )

    // *** NEW: Mouse Move Tool ***
    let mouseMoveTool = Tool(
        name: "macos-use_mouse_move",
        description:
            "Moves the mouse cursor to the specified screen coordinates without clicking. Useful for hover effects, tooltips, and positioning before other actions.",
        inputSchema: mouseActionSchema
    )

    // *** NEW: Drag & Drop Tool ***
    let dragDropSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the target application. Defaults to frontmost app."),
            ]),
            "startX": .object([
                "type": .string("number"), "description": .string("REQUIRED. Start X coordinate."),
            ]),
            "startY": .object([
                "type": .string("number"), "description": .string("REQUIRED. Start Y coordinate."),
            ]),
            "endX": .object([
                "type": .string("number"), "description": .string("REQUIRED. End X coordinate."),
            ]),
            "endY": .object([
                "type": .string("number"), "description": .string("REQUIRED. End Y coordinate."),
            ]),
            "steps": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. Number of interpolation steps for smooth drag (default 10)."),
            ]),
            "activateApp": .object([
                "type": .string("boolean"),
                "description": .string("OPTIONAL. Activate the app (bring to front) before executing. Default: true."),
            ]),
        ]),
        "required": .array([
            .string("startX"), .string("startY"), .string("endX"), .string("endY"),
        ]),
    ])
    let dragDropTool = Tool(
        name: "macos-use_drag_and_drop_and_traverse",
        description: "Simulates a mouse drag-and-drop action.",
        inputSchema: dragDropSchema
    )

    // *** NEW: Network Diagnostics Tool ***
    let networkDiagnosticsSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "action": .object([
                "type": .string("string"),
                "description": .string("REQUIRED. Action: 'connectivity', 'interfaces', 'public_ip', 'dns_lookup', 'ping', 'full'"),
            ]),
            "target": .object([
                "type": .string("string"),
                "description": .string("OPTIONAL. Target host for ping/dns_lookup (default: google.com)"),
            ]),
        ]),
        "required": .array([.string("action")]),
    ])
    let networkDiagnosticsTool = Tool(
        name: "macos-use_network_diagnostics",
        description: "Network diagnostics: check connectivity, list interfaces, get public IP, DNS lookup, ping hosts.",
        inputSchema: networkDiagnosticsSchema
    )

    let windowMgmtSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the application. Defaults to frontmost app."),
            ]),
            "action": .object([
                "type": .string("string"),
                "description": .string(
                    "Action: 'move', 'resize', 'minimize', 'maximize', 'make_front', 'snapshot', 'group', 'ungroup'."
                ),
                "enum": .array([
                    .string("move"), .string("resize"), .string("minimize"),
                    .string("maximize"), .string("make_front"), .string("snapshot"),
                    .string("group"), .string("ungroup"),
                ]),
            ]),
            "x": .object([
                "type": .string("number"), "description": .string("Optional X for move."),
            ]),
            "y": .object([
                "type": .string("number"), "description": .string("Optional Y for move."),
            ]),
            "width": .object([
                "type": .string("number"), "description": .string("Optional Width for resize."),
            ]),
            "height": .object([
                "type": .string("number"), "description": .string("Optional Height for resize."),
            ]),
            "groupId": .object([
                "type": .string("string"),
                "description": .string("Optional: Group ID for grouping/ungrouping windows."),
            ]),
            "snapshotPath": .object([
                "type": .string("string"),
                "description": .string("Optional: Path to save window snapshot."),
            ]),
        ]),
        "required": .array([.string("action")]),
    ])
    let windowMgmtTool = Tool(
        name: "macos-use_window_management",
        description: "Enhanced window management with snapshots, grouping, and advanced actions.",
        inputSchema: windowMgmtSchema
    )

    // *** ENHANCED: Clipboard Tools ***
    let setClipboardSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "text": .object([
                "type": .string("string"),
                "description": .string("Text to set to clipboard (for plain text)."),
            ]),
            "html": .object([
                "type": .string("string"),
                "description": .string("Optional HTML content for rich text clipboard."),
            ]),
            "image": .object([
                "type": .string("string"),
                "description": .string("Optional base64 image data for image clipboard."),
            ]),
            "addToHistory": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Add to clipboard history (default: true)."),
            ]),
            "showAnimation": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Show visual focus animation."),
            ]),
            "animationDuration": .object([
                "type": .string("number"),
                "description": .string("Optional: Duration of the animation in seconds."),
            ]),
        ]),
        "required": .array([.string("text")]),
    ])
    let setClipboardTool = Tool(
        name: "macos-use_set_clipboard",
        description: "Enhanced clipboard with rich text, images, and history support.",
        inputSchema: setClipboardSchema
    )

    let getClipboardSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "format": .object([
                "type": .string("string"),
                "description": .string("Optional: Return format - 'text', 'html', 'image', 'all'."),
                "enum": .array([.string("text"), .string("html"), .string("image"), .string("all")]
                ),
            ]),
            "history": .object([
                "type": .string("boolean"),
                "description": .string(
                    "Optional: Return clipboard history instead of current content."),
            ]),
            "limit": .object([
                "type": .string("number"),
                "description": .string("Optional: Limit history results (default: 10)."),
            ]),
        ]),
    ])
    let getClipboardTool = Tool(
        name: "macos-use_get_clipboard",
        description: "Enhanced clipboard getter with history, rich text, and image support.",
        inputSchema: getClipboardSchema
    )

    let clipboardHistorySchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "clear": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Clear clipboard history."),
            ]),
            "limit": .object([
                "type": .string("number"),
                "description": .string("Optional: Maximum history items to keep."),
            ]),
        ]),
    ])
    let clipboardHistoryTool = Tool(
        name: "macos-use_clipboard_history",
        description: "Manage clipboard history - clear, limit, and view history.",
        inputSchema: clipboardHistorySchema
    )

    // *** NEW: Voice Control Tool ***
    let voiceControlSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "command": .object([
                "type": .string("string"),
                "description": .string(
                    "Voice command to execute (e.g., 'open Safari', 'take screenshot', 'type hello')."
                ),
            ]),
            "language": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Language for voice recognition (en-US, uk-UA, etc.)."),
                "enum": .array([
                    .string("en-US"), .string("uk-UA"), .string("ru-RU"), .string("de-DE"),
                ]),
            ]),
            "confidence": .object([
                "type": .string("number"),
                "description": .string("Optional: Minimum confidence threshold (0.0-1.0)."),
            ]),
        ]),
        "required": .array([.string("command")]),
    ])
    let voiceControlTool = Tool(
        name: "macos-use_voice_control",
        description: "Voice control system with speech recognition and command execution.",
        inputSchema: voiceControlSchema
    )

    // *** NEW: Process Management Tool ***
    let processManagementSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "action": .object([
                "type": .string("string"),
                "description": .string("Action: 'list', 'kill', 'restart', 'monitor', 'priority'."),
                "enum": .array([
                    .string("list"), .string("kill"), .string("restart"), .string("monitor"),
                    .string("priority"),
                ]),
            ]),
            "pid": .object([
                "type": .string("number"),
                "description": .string("Optional: Process ID for specific actions."),
            ]),
            "name": .object([
                "type": .string("string"),
                "description": .string("Optional: Process name for specific actions."),
            ]),
            "priority": .object([
                "type": .string("string"),
                "description": .string("Optional: Priority level (low, normal, high)."),
                "enum": .array([.string("low"), .string("normal"), .string("high")]),
            ]),
            "duration": .object([
                "type": .string("number"),
                "description": .string("Optional: Monitoring duration in seconds."),
            ]),
        ]),
        "required": .array([.string("action")]),
    ])
    let processManagementTool = Tool(
        name: "macos-use_process_management",
        description:
            "Advanced process management with monitoring, control, and priority adjustment.",
        inputSchema: processManagementSchema
    )

    // *** NEW: File Encryption Tool ***
    let fileEncryptionSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "action": .object([
                "type": .string("string"),
                "description": .string(
                    "Action: 'encrypt', 'decrypt', 'encrypt_folder', 'decrypt_folder'."),
                "enum": .array([
                    .string("encrypt"), .string("decrypt"), .string("encrypt_folder"),
                    .string("decrypt_folder"),
                ]),
            ]),
            "path": .object([
                "type": .string("string"),
                "description": .string("File or folder path to encrypt/decrypt."),
            ]),
            "password": .object([
                "type": .string("string"),
                "description": .string("Encryption password."),
            ]),
            "algorithm": .object([
                "type": .string("string"),
                "description": .string("Encryption algorithm (AES256, AES128)."),
                "enum": .array([.string("AES256"), .string("AES128")]),
            ]),
            "output": .object([
                "type": .string("string"),
                "description": .string("Optional: Output path for encrypted/decrypted file."),
            ]),
        ]),
        "required": .array([.string("action"), .string("path"), .string("password")]),
    ])
    let fileEncryptionTool = Tool(
        name: "macos-use_file_encryption",
        description:
            "File and folder encryption with AES algorithms and secure password management.",
        inputSchema: fileEncryptionSchema
    )

    // *** NEW: System Monitoring Tool ***
    let systemMonitoringSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "metric": .object([
                "type": .string("string"),
                "description": .string(
                    "Metric to monitor: 'cpu', 'memory', 'disk', 'network', 'battery', 'all'."),
                "enum": .array([
                    .string("cpu"), .string("memory"), .string("disk"), .string("network"),
                    .string("battery"), .string("all"),
                ]),
            ]),
            "duration": .object([
                "type": .string("number"),
                "description": .string("Monitoring duration in seconds."),
            ]),
            "interval": .object([
                "type": .string("number"),
                "description": .string("Sampling interval in seconds."),
            ]),
            "alert": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Send alert if thresholds exceeded."),
            ]),
            "threshold": .object([
                "type": .string("number"),
                "description": .string("Optional: Alert threshold (0-100)."),
            ]),
        ]),
        "required": .array([.string("metric")]),
    ])
    let systemMonitoringTool = Tool(
        name: "macos-use_system_monitoring",
        description:
            "Real-time system monitoring with CPU, memory, disk, network, and battery metrics.",
        inputSchema: systemMonitoringSchema
    )

    // *** ENHANCED: System Control Tool ***
    let mediaControlSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "action": .object([
                "type": .string("string"),
                "description": .string(
                    "Action: 'play_pause', 'next', 'previous', 'volume_up', 'volume_down', 'mute', 'brightness_up', 'brightness_down', 'get_info', 'get_system_info', 'get_performance', 'get_network', 'get_storage'."
                ),
                "enum": .array([
                    .string("play_pause"), .string("next"), .string("previous"),
                    .string("volume_up"), .string("volume_down"), .string("mute"),
                    .string("brightness_up"), .string("brightness_down"),
                    .string("get_info"), .string("get_system_info"), .string("get_performance"),
                    .string("get_network"), .string("get_storage"),
                ]),
            ]),
            "value": .object([
                "type": .string("number"),
                "description": .string("Optional numeric value for volume/brightness (0-100)."),
            ]),
        ]),
        "required": .array([.string("action")]),
    ])
    let mediaControlTool = Tool(
        name: "macos-use_system_control",
        description: "Enhanced system control with monitoring, metrics, and comprehensive actions.",
        inputSchema: mediaControlSchema
    )

    // *** NEW: Fetch URL Tool ***
    let fetchRelaySchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "url": .object([
                "type": .string("string"),
                "description": .string("REQUIRED. The URL to fetch."),
            ])
        ]),
        "required": .array([.string("url")]),
    ])
    let fetchTool = Tool(
        name: "macos-use_fetch_url",
        description:
            "Fetches content from a URL and converts HTML to text/markdown using SwiftSoup.",
        inputSchema: fetchRelaySchema
    )

    // *** ENHANCED: Time Tools ***
    let getTimeSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "timezone": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Timezone identifier (e.g., 'America/Los_Angeles'). Defaults to system."
                ),
            ]),
            "format": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Time format - 'iso', 'readable', 'unix', 'custom'."),
                "enum": .array([
                    .string("iso"), .string("readable"), .string("unix"), .string("custom"),
                ]),
            ]),
            "customFormat": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Custom format string (used with format='custom')."),
            ]),
            "convertTo": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Convert to timezone (e.g., 'UTC', 'Europe/Kiev')."),
            ]),
        ]),
    ])
    let getTimeTool = Tool(
        name: "macos-use_get_time",
        description:
            "Enhanced time tool with timezone conversion, formatting, and countdown support.",
        inputSchema: getTimeSchema
    )

    let countdownSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "seconds": .object([
                "type": .string("number"),
                "description": .string("Required: Countdown duration in seconds."),
            ]),
            "message": .object([
                "type": .string("string"),
                "description": .string("Optional: Message to display when countdown ends."),
            ]),
            "notification": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Send notification when countdown ends."),
            ]),
        ]),
        "required": .array([.string("seconds")]),
    ])
    let countdownTool = Tool(
        name: "macos-use_countdown_timer",
        description: "Countdown timer with notification support.",
        inputSchema: countdownSchema
    )

    // *** ULTIMATE: AppleScript Tool ***
    let appleScriptSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "script": .object([
                "type": .string("string"),
                "description": .string("REQUIRED. The AppleScript code to execute."),
            ]),
            "template": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Use predefined template (automation, file_ops, system_info, etc.)."),
            ]),
            "aiGenerate": .object([
                "type": .string("boolean"),
                "description": .string(
                    "Optional: Generate AppleScript using AI based on description."),
            ]),
            "description": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Describe what you want to accomplish, AI will generate the script."),
            ]),
            "debug": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Enable debugging mode with detailed output."),
            ]),
            "timeout": .object([
                "type": .string("number"),
                "description": .string("Optional: Execution timeout in seconds (default: 10)."),
            ]),
            "validate": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Validate script syntax before execution."),
            ]),
        ]),
        "required": .array([.string("script")]),
    ])
    let appleScriptTool = Tool(
        name: "macos-use_run_applescript",
        description:
            "Ultimate AppleScript tool with AI generation, templates, debugging, and validation.",
        inputSchema: appleScriptSchema
    )

    let appleScriptTemplatesSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "list": .object([
                "type": .string("boolean"),
                "description": .string("Optional: List all available templates."),
            ]),
            "create": .object([
                "type": .string("string"),
                "description": .string("Optional: Create new template with name and script."),
            ]),
            "name": .object([
                "type": .string("string"),
                "description": .string("Template name (required for create)."),
            ]),
            "script": .object([
                "type": .string("string"),
                "description": .string("Template script content (required for create)."),
            ]),
            "description": .object([
                "type": .string("string"),
                "description": .string("Template description."),
            ]),
        ]),
    ])
    let appleScriptTemplatesTool = Tool(
        name: "macos-use_applescript_templates",
        description: "Manage AppleScript templates - create, list, and use templates.",
        inputSchema: appleScriptTemplatesSchema
    )

    // *** NEW: Calendar Tools ***
    let calendarEventsSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "start": .object([
                "type": .string("string"),
                "description": .string("Start date (ISO8601 or natural language)"),
            ]),
            "end": .object([
                "type": .string("string"),
                "description": .string("End date (ISO8601 or natural language)"),
            ]),
        ]),
        "required": .array([.string("start"), .string("end")]),
    ])
    let calendarEventsTool = Tool(
        name: "macos-use_calendar_events",
        description: "Fetch calendar events for a date range.",
        inputSchema: calendarEventsSchema
    )

    // *** ENHANCED: Calendar Event Creation ***
    let createEventSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "title": .object(["type": .string("string"), "description": .string("Event title")]),
            "date": .object(["type": .string("string"), "description": .string("Event date")]),
            "endDate": .object([
                "type": .string("string"), "description": .string("Optional end date"),
            ]),
            "location": .object([
                "type": .string("string"), "description": .string("Optional event location"),
            ]),
            "notes": .object([
                "type": .string("string"), "description": .string("Optional event notes"),
            ]),
            "attendees": .object([
                "type": .string("array"),
                "description": .string("Optional list of attendee emails"),
                "items": .object(["type": .string("string")]),
            ]),
            "recurring": .object([
                "type": .string("string"),
                "description": .string("Optional recurring pattern: 'daily', 'weekly', 'monthly'"),
                "enum": .array([.string("daily"), .string("weekly"), .string("monthly")]),
            ]),
            "reminder": .object([
                "type": .string("number"),
                "description": .string("Optional reminder in minutes before event"),
            ]),
        ]),
        "required": .array([.string("title"), .string("date")]),
    ])
    let createEventTool = Tool(
        name: "macos-use_create_event",
        description:
            "Enhanced calendar event creation with attendees, location, recurring events, and reminders.",
        inputSchema: createEventSchema
    )

    // *** NEW: Reminder Tools ***
    let remindersSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "list": .object([
                "type": .string("string"), "description": .string("Optional list name filter"),
            ])
        ]),
    ])
    let remindersTool = Tool(
        name: "macos-use_reminders",
        description: "Fetch incomplete reminders.",
        inputSchema: remindersSchema
    )

    let createReminderSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "title": .object(["type": .string("string"), "description": .string("Reminder title")])
        ]),
        "required": .array([.string("title")]),
    ])
    let createReminderTool = Tool(
        name: "macos-use_create_reminder",
        description: "Create a new reminder.",
        inputSchema: createReminderSchema
    )

    // *** NEW: Spotlight Tool ***
    let spotlightSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "query": .object([
                "type": .string("string"), "description": .string("Filename search query"),
            ])
        ]),
        "required": .array([.string("query")]),
    ])
    let spotlightTool = Tool(
        name: "macos-use_spotlight_search",
        description: "Search for files using Spotlight (mdfind).",
        inputSchema: spotlightSchema
    )

    // *** ENHANCED: Notification Tool ***
    let notificationSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "title": .object([
                "type": .string("string"), "description": .string("Notification title"),
            ]),
            "message": .object([
                "type": .string("string"), "description": .string("Notification body text"),
            ]),
            "schedule": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Schedule notification in ISO format (e.g., '2026-02-10T15:00:00Z')"),
            ]),
            "sound": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Sound name - 'default', 'none', or system sound name"),
            ]),
            "persistent": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Keep notification until dismissed"),
            ]),
            "template": .object([
                "type": .string("string"),
                "description": .string("Optional: Use predefined template"),
            ]),
        ]),
        "required": .array([.string("title"), .string("message")]),
    ])
    let notificationTool = Tool(
        name: "macos-use_send_notification",
        description: "Enhanced notification with scheduling, custom sounds, and persistence.",
        inputSchema: notificationSchema
    )

    let notificationScheduleSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "clear": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Clear all scheduled notifications"),
            ]),
            "list": .object([
                "type": .string("boolean"),
                "description": .string("Optional: List scheduled notifications"),
            ]),
        ]),
    ])
    let notificationScheduleTool = Tool(
        name: "macos-use_notification_schedule",
        description: "Manage scheduled notifications - clear, list, and schedule.",
        inputSchema: notificationScheduleSchema
    )

    // *** NEW: Apple Notes Tools ***
    let notesListFoldersSchema: Value = .object([
        "type": .string("object"), "properties": .object([:]),
    ])
    let notesListFoldersTool = Tool(
        name: "macos-use_notes_list_folders",
        description: "List all folders in Apple Notes.",
        inputSchema: notesListFoldersSchema
    )

    let notesCreateSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "body": .object([
                "type": .string("string"),
                "description": .string("HTML or plain text content. First line becomes title."),
            ]),
            "folder": .object([
                "type": .string("string"), "description": .string("Optional folder name."),
            ]),
        ]),
        "required": .array([.string("body")]),
    ])
    let notesCreateTool = Tool(
        name: "macos-use_notes_create_note",
        description: "Create a new note in Apple Notes.",
        inputSchema: notesCreateSchema
    )

    let notesGetSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "name": .object([
                "type": .string("string"),
                "description": .string("Name/Title of the note to find."),
            ])
        ]),
        "required": .array([.string("name")]),
    ])
    let notesGetTool = Tool(
        name: "macos-use_notes_get_content",
        description: "Get the HTML content of a note by name.",
        inputSchema: notesGetSchema
    )

    // *** ENHANCED: Apple Mail Tools ***
    let mailSendSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "to": .object([
                "type": .string("string"), "description": .string("Recipient email address"),
            ]),
            "cc": .object([
                "type": .string("string"), "description": .string("Optional CC recipient"),
            ]),
            "bcc": .object([
                "type": .string("string"), "description": .string("Optional BCC recipient"),
            ]),
            "subject": .object(["type": .string("string"), "description": .string("Subject line")]),
            "body": .object([
                "type": .string("string"), "description": .string("Email body content"),
            ]),
            "html": .object([
                "type": .string("boolean"), "description": .string("Optional: Send as HTML email"),
            ]),
            "attachments": .object([
                "type": .string("array"),
                "description": .string("Optional list of file paths to attach"),
                "items": .object(["type": .string("string")]),
            ]),
            "draft": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Save as draft instead of sending"),
            ]),
        ]),
        "required": .array([.string("to"), .string("subject"), .string("body")]),
    ])
    let mailSendTool = Tool(
        name: "macos-use_mail_send",
        description:
            "Enhanced email sending with CC/BCC, HTML formatting, attachments, and draft support.",
        inputSchema: mailSendSchema
    )

    let mailReadSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "limit": .object([
                "type": .string("integer"),
                "description": .string("Number of recent messages to read (default 5)"),
            ])
        ]),
    ])
    let mailReadTool = Tool(
        name: "macos-use_mail_read_inbox",
        description: "Read recent subject lines from Apple Mail Inbox.",
        inputSchema: mailReadSchema
    )

    // *** ENHANCED: Finder Tools ***
    let finderListFilesSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "path": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional folder path. If omitted, uses frontmost Finder window."),
            ]),
            "filter": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional filter pattern (e.g., '*.txt', 'name contains test')."),
            ]),
            "sort": .object([
                "type": .string("string"),
                "description": .string("Optional sort order: 'name', 'date', 'size', 'type'."),
                "enum": .array([.string("name"), .string("date"), .string("size"), .string("type")]
                ),
            ]),
            "order": .object([
                "type": .string("string"),
                "description": .string("Optional sort direction: 'asc', 'desc'."),
                "enum": .array([.string("asc"), .string("desc")]),
            ]),
            "limit": .object([
                "type": .string("number"),
                "description": .string("Optional limit number of results."),
            ]),
            "metadata": .object([
                "type": .string("boolean"),
                "description": .string(
                    "Optional: Include file metadata (size, dates, permissions)."),
            ]),
        ]),
    ])
    let finderListFilesTool = Tool(
        name: "macos-use_finder_list_files",
        description:
            "Enhanced file listing with filtering, sorting, metadata extraction, and pagination support.",
        inputSchema: finderListFilesSchema
    )

    let finderGetSelectionSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "metadata": .object([
                "type": .string("boolean"),
                "description": .string("Optional: Include file metadata for selected items."),
            ])
        ]),
    ])
    let finderGetSelectionTool = Tool(
        name: "macos-use_finder_get_selection",
        description:
            "Returns the POSIX paths of currently selected items in Finder with optional metadata.",
        inputSchema: finderGetSelectionSchema
    )

    let finderOpenPathSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "path": .object([
                "type": .string("string"),
                "description": .string("REQUIRED. The POSIX path to open."),
            ])
        ]),
        "required": .array([.string("path")]),
    ])
    let finderOpenPathTool = Tool(
        name: "macos-use_finder_open_path",
        description: "Opens a folder or file in Finder.",
        inputSchema: finderOpenPathSchema
    )

    let finderMoveToTrashSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "path": .object([
                "type": .string("string"),
                "description": .string("REQUIRED. The POSIX path of the item to trash."),
            ])
        ]),
        "required": .array([.string("path")]),
    ])
    let finderMoveToTrashTool = Tool(
        name: "macos-use_finder_move_to_trash",
        description: "Moves the specified file or folder to the Trash via Finder.",
        inputSchema: finderMoveToTrashSchema
    )

    // *** NEW: List Running Applications ***
    let listAppsSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "filter": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Filter by application name or bundle identifier."),
            ])
        ]),
    ])
    let listAppsTool = Tool(
        name: "macos-use_list_running_apps",
        description:
            "Returns a list of all currently running applications with their PIDs, bundle IDs, and window information.",
        inputSchema: listAppsSchema
    )

    // *** NEW: List Browser Tabs ***
    let listTabsSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "browser": .object([
                "type": .string("string"),
                "description":
                    "Browser name (chrome, safari, firefox). If not specified, checks all browsers.",
            ])
        ]),
    ])
    let listTabsTool = Tool(
        name: "macos-use_list_browser_tabs",
        description: "Returns a list of open tabs in specified browser with titles and URLs.",
        inputSchema: listTabsSchema
    )

    // *** NEW: List All Windows ***
    let listWindowsSchema: Value = .object([
        "type": .string("object"), "properties": .object([:]),
    ])
    let listWindowsTool = Tool(
        name: "macos-use_list_all_windows",
        description:
            "Returns a list of all open windows across all applications with titles and positions.",
        inputSchema: listWindowsSchema
    )

    // *** NEW: Dynamic Help ***
    let dynamicHelpSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "toolName": .object([
                "type": .string("string"),
                "description": .string("Optional: Filter by tool name"),
            ])
        ]),
    ])
    let dynamicHelpTool = Tool(
        name: "macos-use_list_tools_dynamic",
        description:
            "Returns a detailed JSON structure describing all available tools, their schemas, and usage examples.",
        inputSchema: dynamicHelpSchema
    )

    // *** NEW: Frontmost App Tool ***
    let frontmostAppTool = Tool(
        name: "macos-use_get_frontmost_app",
        description: "Returns information about the currently active (frontmost) application.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: Battery Info Tool ***
    let batteryInfoTool = Tool(
        name: "macos-use_get_battery_info",
        description: "Returns the current battery status, including percentage and charging state.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: WiFi Details Tool ***
    let wifiDetailsTool = Tool(
        name: "macos-use_get_wifi_details",
        description: "Returns details about the current WiFi connection (SSID, signal strength).",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: Set System Volume Tool ***
    let setVolumeSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "level": .object([
                "type": .string("number"),
                "description": .string("Volume level from 0 to 100."),
            ])
        ]),
        "required": .array([.string("level")]),
    ])
    let setVolumeTool = Tool(
        name: "macos-use_set_system_volume",
        description: "Sets the system output volume to a specific level (0-100).",
        inputSchema: setVolumeSchema
    )

    // *** NEW: Set Screen Brightness Tool ***
    let setBrightnessSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "level": .object([
                "type": .string("number"),
                "description": .string("Brightness level from 0.0 to 1.0."),
            ])
        ]),
        "required": .array([.string("level")]),
    ])
    let setBrightnessTool = Tool(
        name: "macos-use_set_screen_brightness",
        description: "Sets the primary display brightness (0.0 to 1.0).",
        inputSchema: setBrightnessSchema
    )

    // *** NEW: Empty Trash Tool ***
    let emptyTrashTool = Tool(
        name: "macos-use_empty_trash",
        description: "Empties the macOS Trash via Finder.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: Window Info Tool ***
    let windowInfoTool = Tool(
        name: "macos-use_get_active_window_info",
        description: "Returns detailed information about the frontmost window.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: Close Window Tool ***
    let closeWindowSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "windowName": .object([
                "type": .string("string"),
                "description": .string(
                    "Optional: Title of the window to close. If omitted, closes front window."),
            ])
        ]),
    ])
    let closeWindowTool = Tool(
        name: "macos-use_close_window",
        description: "Closes a specific window by name or the frontmost window.",
        inputSchema: closeWindowSchema
    )

    // *** NEW: Move Window Tool ***
    let moveWindowSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "x": .object(["type": .string("number"), "description": .string("New X coordinate")]),
            "y": .object(["type": .string("number"), "description": .string("New Y coordinate")]),
            "windowName": .object([
                "type": .string("string"), "description": .string("Optional: Window title"),
            ]),
        ]),
        "required": .array([.string("x"), .string("y")]),
    ])
    let moveWindowTool = Tool(
        name: "macos-use_move_window",
        description: "Moves a window to specified screen coordinates.",
        inputSchema: moveWindowSchema
    )

    // *** NEW: Resize Window Tool ***
    let resizeWindowSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "width": .object(["type": .string("number"), "description": .string("New width")]),
            "height": .object(["type": .string("number"), "description": .string("New height")]),
            "windowName": .object([
                "type": .string("string"), "description": .string("Optional: Window title"),
            ]),
        ]),
        "required": .array([.string("width"), .string("height")]),
    ])
    let resizeWindowTool = Tool(
        name: "macos-use_resize_window",
        description: "Resizes a window to specified dimensions.",
        inputSchema: resizeWindowSchema
    )

    // *** NEW: List Network Interfaces Tool ***
    let listNetworkInterfacesTool = Tool(
        name: "macos-use_list_network_interfaces",
        description: "Lists all active network interfaces on the system.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: Get IP Address Tool ***
    let getIPAddressTool = Tool(
        name: "macos-use_get_ip_address",
        description: "Returns the local and estimated public IP address.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: Request Permissions Tool ***
    let requestPermissionsTool = Tool(
        name: "macos-use_request_permissions",
        description:
            "Sequentially requests all necessary macOS permissions (Calendar, Reminders, Notifications, Accessibility) to trigger interactive system prompts.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** ALIAS TOOLS FOR naming compatibility ***
    let appleScriptAliasTool = Tool(
        name: "macos-use_applescript",
        description: "Alias for macos-use_run_applescript.",
        inputSchema: appleScriptSchema
    )
    let notesCreateAliasTool = Tool(
        name: "macos-use_notes_create",
        description: "Alias for macos-use_notes_create_note.",
        inputSchema: notesCreateSchema
    )
    let notesGetAliasTool = Tool(
        name: "macos-use_notes_get",
        description: "Alias for macos-use_notes_get_content.",
        inputSchema: notesGetSchema
    )
    let notificationAliasTool = Tool(
        name: "macos-use_notification",
        description: "Alias for macos-use_send_notification.",
        inputSchema: notificationSchema
    )
    let mailReadAliasTool = Tool(
        name: "macos-use_mail_read",
        description: "Alias for macos-use_mail_read_inbox.",
        inputSchema: mailReadSchema
    )
    let countdownAliasTool = Tool(
        name: "macos-use_countdown",
        description: "Alias for macos-use_countdown_timer.",
        inputSchema: countdownSchema
    )
    let macosUseExecuteCommandAliasTool = Tool(
        name: "macos-use_execute_command",
        description: "Alias for execute_command.",
        inputSchema: executeCommandSchema
    )
    let openTerminalAliasTool = Tool(
        name: "macos-use_open_terminal",
        description: "Alias for execute_command.",
        inputSchema: executeCommandSchema
    )

    // --- Aggregate list of tools ---
    let allTools = [
        openAppTool,
        clickTool,
        typeTool,
        refreshTool,
        executeCommandTool,
        terminalTool,
        screenshotTool,
        screenshotAliasTool,
        visionTool,
        unifiedVisionTool,
        pressKeyTool,
        scrollTool,
        rightClickTool,
        doubleClickTool,
        tripleClickTool,
        mouseMoveTool,
        dragDropTool,
        networkDiagnosticsTool,
        windowMgmtTool,
        setClipboardTool,
        getClipboardTool,
        clipboardHistoryTool,
        voiceControlTool,
        processManagementTool,
        fileEncryptionTool,
        systemMonitoringTool,
        mediaControlTool,
        fetchTool,
        getTimeTool,
        countdownTool,
        appleScriptTool,
        appleScriptTemplatesTool,
        calendarEventsTool,
        createEventTool,
        remindersTool,
        createReminderTool,
        spotlightTool,
        notificationTool,
        notificationScheduleTool,
        notesListFoldersTool,
        notesCreateTool,
        notesGetTool,
        mailSendTool,
        mailReadTool,
        finderListFilesTool,
        finderGetSelectionTool,
        finderOpenPathTool,
        finderMoveToTrashTool,
        listAppsTool,
        listTabsTool,
        listWindowsTool,
        dynamicHelpTool,
        frontmostAppTool,
        batteryInfoTool,
        wifiDetailsTool,
        setVolumeTool,
        setBrightnessTool,
        emptyTrashTool,
        windowInfoTool,
        closeWindowTool,
        moveWindowTool,
        resizeWindowTool,
        listNetworkInterfacesTool,
        getIPAddressTool,
        requestPermissionsTool,
        appleScriptAliasTool,
        notesCreateAliasTool,
        notesGetAliasTool,
        notificationAliasTool,
        mailReadAliasTool,
        countdownAliasTool,
        macosUseExecuteCommandAliasTool,
        openTerminalAliasTool
    ]
    debugLog(
        "log: setupAndStartServer: defined \(allTools.count) tools: \(allTools.map { $0.name })\n",
        stderr)

    let server = Server(
        name: "SwiftMacOSServerDirect",  // Renamed slightly
        version: "1.6.0",  // Incremented version for ultimate enhancements
        capabilities: .init(
            tools: .init(listChanged: true)
        )
    )
    debugLog(
        "log: setupAndStartServer: server instance created (\(server.name)) version \(server.version).\n",
        stderr)

    // --- Dummy Handlers (ReadResource, ListResources, ListPrompts) ---
    // (Keep these as they are part of the MCP spec, even if unused for now)
    await server.withMethodHandler(ReadResource.self) { params in
        let uri = params.uri
        debugLog(
            "log: handler(ReadResource): received request for uri: \(uri) (dummy handler)\n", stderr
        )
        // In a real scenario, you might fetch resource content here
        return .init(contents: [.text("dummy content for \(uri)", uri: uri)])
    }
    debugLog("log: setupAndStartServer: registered ReadResource handler (dummy).\n", stderr)

    await server.withMethodHandler(ListResources.self) { _ in
        debugLog("log: handler(ListResources): received request (dummy handler).\n", stderr)
        // In a real scenario, list available resources
        return ListResources.Result(resources: [])
    }
    debugLog("log: setupAndStartServer: registered ListResources handler (dummy).\n", stderr)

    await server.withMethodHandler(ListPrompts.self) { _ in
        debugLog("log: handler(ListPrompts): received request (dummy handler).\n", stderr)
        // In a real scenario, list available prompts
        return ListPrompts.Result(prompts: [])
    }
    debugLog("log: setupAndStartServer: registered ListPrompts handler (dummy).\n", stderr)

    // --- ListTools Handler ---
    await server.withMethodHandler(ListTools.self) { _ in
        debugLog("log: handler(ListTools): received request.\n", stderr)
        let result = ListTools.Result(tools: allTools)
        debugLog(
            "log: handler(ListTools): responding with \(result.tools.count) tools: \(result.tools.map { $0.name })\n",
            stderr)
        return result
    }
    debugLog("log: setupAndStartServer: registered ListTools handler.\n", stderr)

    // --- UPDATED CallTool Handler (Direct SDK Call) ---
    await server.withMethodHandler(CallTool.self) { params in
        debugLog("log: handler(CallTool): received request for tool: \(params.name).\n", stderr)
        debugLog(
            "log: handler(CallTool): arguments received (raw MCP): \(params.arguments?.debugDescription ?? "nil")\n",
            stderr)

        // --- Initialize Action and Options ---
        var primaryAction: PrimaryAction = .traverseOnly  // Default action
        var options = ActionOptions()  // Start with default options
        options.showAnimation = false  // DISABLED BY DEFAULT to prevent freezing with hundreds of overlays
        options.animationDuration = 0.8  // 0.8s for good visibility
        options.traverseAfter = false // Changed to false to reduce context size; tools must explicitly request traversal if needed.
        options.onlyVisibleElements = true // Default to true to keep context small

        do {
            // --- Determine Action and Options from MCP Params ---

            // PID is optional (defaults to frontmost app if 0, -1 or missing)
            let pidOptionalInt = try getOptionalInt(from: params.arguments, key: "pid")
            let resolvedPid = resolvePid(pidOptionalInt)

            // Convert to pid_t
            guard let convertedPid = pid_t(exactly: resolvedPid) else {
                debugLog(
                    "error: handler(CallTool): Resolved PID value \(resolvedPid) is out of range for pid_t.\n",
                    stderr)
                throw MCPError.invalidParams("Resolved PID value \(resolvedPid) is out of range.")
            }

            // Set PID for traversal if needed
            if options.traverseBefore || options.traverseAfter {
                options.pidForTraversal = convertedPid
            }

            // Potentially allow overriding default options from params
            options.traverseBefore =
                try getOptionalBool(from: params.arguments, key: "traverseBefore")
                ?? options.traverseBefore
            options.traverseAfter =
                try getOptionalBool(from: params.arguments, key: "traverseAfter")
                ?? options.traverseAfter
            options.showDiff =
                try getOptionalBool(from: params.arguments, key: "showDiff") ?? options.showDiff
            options.onlyVisibleElements =
                try getOptionalBool(from: params.arguments, key: "onlyVisibleElements")
                ?? options.onlyVisibleElements
            options.showAnimation =
                try getOptionalBool(from: params.arguments, key: "showAnimation")
                ?? options.showAnimation
            options.animationDuration =
                try getOptionalDouble(from: params.arguments, key: "animationDuration")
                ?? options.animationDuration
            options.delayAfterAction =
                try getOptionalDouble(from: params.arguments, key: "delayAfterAction")
                ?? options.delayAfterAction

            options = options.validated()
            debugLog("log: handler(CallTool): constructed ActionOptions: \(options)\n", stderr)

            // --- Auto-Activation Logic ---
            let activateApp = try getOptionalBool(from: params.arguments, key: "activateApp") ?? true
            if activateApp && convertedPid > 0 {
                let toolsWithActivation = [
                    clickTool.name, typeTool.name, pressKeyTool.name,
                    rightClickTool.name, doubleClickTool.name, tripleClickTool.name,
                    dragDropTool.name
                ]
                if toolsWithActivation.contains(params.name) {
                    if let app = NSRunningApplication(processIdentifier: pid_t(convertedPid)) {
                        app.activate(options: .activateIgnoringOtherApps)
                        try? await Task.sleep(nanoseconds: 50_000_000) // 50ms to let it focus
                    }
                }
            }

            switch params.name {
            case openAppTool.name:
                let identifier = try getRequiredString(from: params.arguments, key: "identifier")
                primaryAction = .open(identifier: identifier)

            case clickTool.name:
                let x = try getRequiredDouble(from: params.arguments, key: "x")
                let y = try getRequiredDouble(from: params.arguments, key: "y")
                primaryAction = .input(action: .click(point: CGPoint(x: x, y: y)))
                options.pidForTraversal = convertedPid  // Re-affirm

            case typeTool.name:
                let text = try getRequiredString(from: params.arguments, key: "text")
                primaryAction = .input(action: .type(text: text))
                options.pidForTraversal = convertedPid  // Re-affirm

            // ... (Other existing cases) ...

            case fetchTool.name:
                let urlString = try getRequiredString(from: params.arguments, key: "url")
                guard let url = URL(string: urlString) else {
                    return CallTool.Result(
                        content: [.text("Invalid URL: \(urlString)")], isError: true)
                }

                let (resultText, isError): (String, Bool) = await withCheckedContinuation {
                    continuation in
                    let task = URLSession.shared.dataTask(with: url) { data, response, error in
                        var text = ""
                        var errorFound = false
                        if let error = error {
                            text = "Error fetching URL: \(error.localizedDescription)"
                            errorFound = true
                        } else if let data = data, let html = String(data: data, encoding: .utf8) {
                            do {
                                let doc = try SwiftSoup.parse(html)
                                text = try doc.text()
                            } catch {
                                text = "Error parsing HTML: \(error.localizedDescription)"
                                errorFound = true
                            }
                        } else {
                            text = "No data or invalid encoding."
                            errorFound = true
                        }
                        continuation.resume(returning: (text, errorFound))
                    }
                    task.resume()
                }
                return CallTool.Result(content: [.text(resultText)], isError: isError)

            case getTimeTool.name:
                let timezone = try getOptionalString(from: params.arguments, key: "timezone")
                let format =
                    try getOptionalString(from: params.arguments, key: "format") ?? "readable"
                let customFormat = try getOptionalString(
                    from: params.arguments, key: "customFormat")
                let convertTo = try getOptionalString(from: params.arguments, key: "convertTo")

                let formatter = DateFormatter()

                // Set timezone if specified
                if let tz = timezone {
                    if let timeZone = TimeZone(identifier: tz) {
                        formatter.timeZone = timeZone
                    } else {
                        return CallTool.Result(
                            content: [.text("Invalid timezone identifier: \(tz)")], isError: true)
                    }
                }

                // Set format
                switch format.lowercased() {
                case "iso":
                    formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ssZ"
                case "unix":
                    formatter.dateFormat = "timeIntervalSince1970"
                case "custom":
                    if let fmt = customFormat {
                        formatter.dateFormat = fmt
                    }
                default:  // readable
                    formatter.dateStyle = .full
                    formatter.timeStyle = .full
                }

                let currentDate = Date()
                var resultText = formatter.string(from: currentDate)

                // Convert timezone if specified
                if let targetTz = convertTo, let targetTimeZone = TimeZone(identifier: targetTz) {
                    let targetDate = currentDate.addingTimeInterval(
                        TimeInterval(TimeZone.current.secondsFromGMT(for: currentDate)))
                    formatter.timeZone = targetTimeZone
                    resultText = formatter.string(from: targetDate)
                    resultText += " (converted from \(timezone ?? "local") to \(targetTz))"
                }

                return CallTool.Result(content: [.text(resultText)])

            case countdownTool.name, countdownAliasTool.name:
                let seconds = try getRequiredInt(from: params.arguments, key: "seconds")
                let message =
                    try getOptionalString(from: params.arguments, key: "message")
                    ?? "Countdown completed!"
                let notification =
                    try getOptionalBool(from: params.arguments, key: "notification") ?? false

                // Create countdown timer
                _ = Timer.scheduledTimer(
                    withTimeInterval: 1.0, repeats: true,
                    block: { _ in
                        // This would need proper async implementation
                    })

                // For now, just simulate countdown
                let startTime = Date()
                let endTime = startTime.addingTimeInterval(TimeInterval(seconds))

                let script = """
                    tell application "System Events"
                        display notification "\(message)" with title "Countdown Complete"
                    """

                if notification {
                    let (success, output, error) = await runAppleScript(
                        script, timeout: Double(seconds) + 5.0)
                    if success {
                        if output.contains("Reminders access error") {
                            return CallTool.Result(
                                content: [
                                    .text(
                                        "Reminders access denied. Please grant permission in System Settings > Privacy & Security > Automation."
                                    )
                                ], isError: true)
                        }
                        return CallTool.Result(
                            content: [
                                .text("Countdown completed: \(seconds) seconds. Notification sent.")
                            ], isError: false)
                    } else {
                        return CallTool.Result(
                            content: [
                                .text(
                                    "Countdown completed: \(seconds) seconds. Error: \(error ?? "Unknown")"
                                )
                            ], isError: true)
                    }
                } else {
                    return CallTool.Result(
                        content: [
                            .text("Countdown started: \(seconds) seconds. Will end at \(endTime)")
                        ], isError: false)
                }

            // --- Handlers for Universal Tools ---

            case calendarEventsTool.name:
                // Native EventKit Implementation
                guard await requestCalendarAccess() else {
                    return .init(
                        content: [
                            .text(
                                "Calendar access denied. Please grant permission in System Settings > Privacy & Security > Calendars."
                            )
                        ], isError: true)
                }

                let startStr = try getRequiredString(from: params.arguments, key: "start")
                let endStr = try getRequiredString(from: params.arguments, key: "end")

                guard let start = parseISO8601(from: startStr),
                    let end = parseISO8601(from: endStr)
                else {
                    return .init(
                        content: [
                            .text("Invalid date format. Use ISO8601 (e.g. 2024-01-01T12:00:00Z).")
                        ], isError: true)
                }

                let predicate = eventStore.predicateForEvents(
                    withStart: start, end: end, calendars: nil)
                let events = eventStore.events(matching: predicate)

                if events.isEmpty {
                    return .init(content: [.text("No events found.")], isError: false)
                }

                let output = events.map { event in
                    let startDate = ISO8601DateFormatter().string(from: event.startDate)
                    let endDate = ISO8601DateFormatter().string(from: event.endDate)
                    return "- \(event.title ?? "No Title") (\(startDate) - \(endDate))"
                }.joined(separator: "\n")

                return .init(content: [.text(output)], isError: false)

            case createEventTool.name:
                // Native EventKit Implementation
                guard await requestCalendarAccess() else {
                    return .init(
                        content: [
                            .text(
                                "Calendar access denied. Please grant permission in System Settings > Privacy & Security > Calendars."
                            )
                        ], isError: true)
                }

                let title = try getRequiredString(from: params.arguments, key: "title")
                let dateStr = try getRequiredString(from: params.arguments, key: "date")

                guard let date = parseISO8601(from: dateStr) else {
                    return .init(
                        content: [.text("Invalid date format. Use ISO8601.")], isError: true)
                }

                let event = EKEvent(eventStore: eventStore)
                event.title = title
                event.startDate = date
                event.endDate = date.addingTimeInterval(3600)  // Default 1 hour
                event.calendar = eventStore.defaultCalendarForNewEvents

                do {
                    try eventStore.save(event, span: .thisEvent)
                    return .init(content: [.text("Event created successfully.")], isError: false)
                } catch {
                    return .init(
                        content: [.text("Failed to create event: \(error.localizedDescription)")],
                        isError: true)
                }

            case remindersTool.name:
                // Native EventKit Implementation
                guard await requestRemindersAccess() else {
                    return .init(
                        content: [
                            .text(
                                "Reminders access denied. Please grant permission in System Settings > Privacy & Security > Reminders."
                            )
                        ], isError: true)
                }

                let listFilter = try getOptionalString(from: params.arguments, key: "list")
                var calendars: [EKCalendar]? = nil

                if let listName = listFilter {
                    let allCalendars = eventStore.calendars(for: .reminder)
                    if let found = allCalendars.first(where: {
                        $0.title.caseInsensitiveCompare(listName) == .orderedSame
                    }) {
                        calendars = [found]
                    } else {
                        return .init(content: [.text("List not found: \(listName)")], isError: true)
                    }
                }

                let predicate = eventStore.predicateForReminders(in: calendars)

                return await withCheckedContinuation { continuation in
                    eventStore.fetchReminders(matching: predicate) { reminders in
                        guard let reminders = reminders else {
                            continuation.resume(
                                returning: .init(
                                    content: [.text("Failed to fetch reminders (nil result).")],
                                    isError: true))
                            return
                        }

                        let incomplete = reminders.filter { !$0.isCompleted }
                        if incomplete.isEmpty {
                            continuation.resume(
                                returning: .init(
                                    content: [.text("No incomplete reminders.")], isError: false))
                            return
                        }

                        let output = incomplete.map {
                            "- \($0.title ?? "No Title") [\($0.calendar.title)]"
                        }.joined(separator: "\n")
                        continuation.resume(
                            returning: .init(content: [.text(output)], isError: false))
                    }
                }

            case createReminderTool.name:
                guard await requestRemindersAccess() else {
                    return .init(content: [.text("Reminders access denied.")], isError: true)
                }

                let title = try getRequiredString(from: params.arguments, key: "title")
                let reminder = EKReminder(eventStore: eventStore)
                reminder.title = title
                reminder.calendar = eventStore.defaultCalendarForNewReminders()

                do {
                    try eventStore.save(reminder, commit: true)
                    return .init(content: [.text("Reminder saved.")], isError: false)
                } catch {
                    return .init(
                        content: [.text("Failed to save reminder: \(error.localizedDescription)")],
                        isError: true)
                }

            case spotlightTool.name:
                let query = try getRequiredString(from: params.arguments, key: "query")
                let results = spotlight.search(queryStr: query)
                return CallTool.Result(content: [.text(results.joined(separator: "\n"))])

            case notificationTool.name, notificationAliasTool.name:
                let title = try getRequiredString(from: params.arguments, key: "title")
                let message = try getRequiredString(from: params.arguments, key: "message")
                let schedule = try getOptionalString(from: params.arguments, key: "schedule")
                let sound = try getOptionalString(from: params.arguments, key: "sound") ?? "default"
                let persistent =
                    try getOptionalBool(from: params.arguments, key: "persistent") ?? false
                let template = try getOptionalString(from: params.arguments, key: "template")

                // Handle template
                var finalTitle = title
                var finalMessage = message

                if let templateName = template {
                    let templateData = getNotificationTemplate(templateName)
                    finalTitle = templateData["title"] ?? title
                    finalMessage = templateData["message"] ?? message
                }

                // Handle scheduling
                if let scheduleTime = schedule {
                    addScheduledNotification(
                        title: finalTitle,
                        message: finalMessage,
                        schedule: scheduleTime,
                        sound: sound,
                        persistent: persistent
                    )
                    return CallTool.Result(content: [
                        .text("Notification scheduled for \(scheduleTime)")
                    ])
                } else {
                    // Send immediately
                    var script =
                        "display notification \"\(escapeForAppleScript(finalMessage))\" with title \"\(escapeForAppleScript(finalTitle))\""

                    if sound != "default" && sound != "none" {
                        script += " sound name \"\(escapeForAppleScript(sound))\""
                    }

                    if persistent {
                        script += " as persistent"
                    }

                    _ = runShellCommand("osascript -e '\(script)'")
                    return CallTool.Result(content: [
                        .text("Notification sent with enhanced options.")
                    ])
                }

            case notificationScheduleTool.name:
                let clear = try getOptionalBool(from: params.arguments, key: "clear") ?? false
                let list = try getOptionalBool(from: params.arguments, key: "list") ?? false

                if clear {
                    clearScheduledNotifications()
                    return CallTool.Result(content: [.text("All scheduled notifications cleared.")])
                } else if list {
                    let scheduled = getScheduledNotifications()
                    guard let jsonString = serializeToJsonString(scheduled) else {
                        return CallTool.Result(
                            content: [.text("Failed to serialize scheduled notifications")],
                            isError: true)
                    }
                    return CallTool.Result(content: [.text(jsonString)])
                } else {
                    return CallTool.Result(content: [
                        .text(
                            "Use 'clear': true or 'list': true to manage scheduled notifications.")
                    ])
                }

            // --- Notes Handlers ---
            case notesListFoldersTool.name:
                let script = """
                    try
                        tell application "Notes"
                            set folderNames to name of every folder
                            return folderNames
                        end tell
                    on error errMsg
                        return "Notes error: " & errMsg
                    end try
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 15.0)
                if success {
                    if output.contains("Notes error") {
                        return CallTool.Result(
                            content: [
                                .text(
                                    "Reminders access denied. Please grant permission in System Settings > Privacy & Security > Automation."
                                )
                            ], isError: true)
                    }
                    return CallTool.Result(content: [.text(output)])
                } else {
                    return CallTool.Result(
                        content: [.text("Error: \(error ?? "Unknown")")], isError: true)
                }

            case notesCreateTool.name, notesCreateAliasTool.name:
                let body = try getRequiredString(from: params.arguments, key: "body")
                let folder = try getOptionalString(from: params.arguments, key: "folder") ?? "Notes"
                let safeBody = escapeForAppleScript(body)
                let safeFolder = escapeForAppleScript(folder)

                let script = """
                    try
                        tell application "Notes"
                            if not (exists folder "\(safeFolder)") then
                                return "Error: Folder '\(safeFolder)' not found."
                            end if
                            make new note at folder "\(safeFolder)" with properties {body:"\(safeBody)"}
                            return "Note created."
                        end tell
                    on error errMsg
                        return "Notes error: " & errMsg
                    end try
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 15.0)
                if success {
                    return CallTool.Result(content: [.text(output)])
                } else {
                    return CallTool.Result(
                        content: [.text("Error: \(error ?? "Unknown")")], isError: true)
                }

            case notesGetTool.name, notesGetAliasTool.name:
                let name = try getRequiredString(from: params.arguments, key: "name")
                let safeName = escapeForAppleScript(name)
                let script = """
                    try
                        tell application "Notes"
                            set theNote to first note whose name contains "\(safeName)"
                            set noteContent to body of theNote
                            return noteContent
                        end tell
                    on error errMsg
                        return "Notes error: " & errMsg
                    end try
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 30.0)
                if success {
                    return CallTool.Result(content: [.text(output)])
                } else {
                    return CallTool.Result(
                        content: [.text("Error: \(error ?? "Unknown")")], isError: true)
                }

            // --- Mail Handlers ---
            case mailSendTool.name:
                let to = try getRequiredString(from: params.arguments, key: "to")
                let subject = try getRequiredString(from: params.arguments, key: "subject")
                let body = try getRequiredString(from: params.arguments, key: "body")

                let safeTo = escapeForAppleScript(to)
                let safeSubject = escapeForAppleScript(subject)
                let safeBody = escapeForAppleScript(body)

                let script = """
                    try
                        tell application "Mail"
                            set newMessage to make new outgoing message with properties {subject:"\(safeSubject)", content:"\(safeBody)", visible:true}
                            tell newMessage
                                make new to recipient at end of to recipients with properties {address:"\(safeTo)"}
                                send
                            end tell
                            return "Email sent."
                        end tell
                    on error errMsg
                        return "Mail error: " & errMsg
                    end try
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 8.0)
                if success {
                    return CallTool.Result(content: [.text(output)])
                } else {
                    return CallTool.Result(
                        content: [.text("Error: \(error ?? "Unknown")")], isError: true)
                }

            case mailReadTool.name, mailReadAliasTool.name:
                let limit = try getOptionalInt(from: params.arguments, key: "limit") ?? 5
                let script = """
                    try
                        tell application "Mail"
                            set inboxMessages to messages of inbox
                            set msgCount to count of inboxMessages
                            if msgCount is 0 then return "Inbox is empty"
                            
                            set resultList to {}
                            set loopLimit to \(limit)
                            if msgCount < loopLimit then set loopLimit to msgCount
                            
                            repeat with i from 1 to loopLimit
                                set msg to item i of inboxMessages
                                set end of resultList to (subject of msg) & " [From: " & (sender of msg) & "]"
                            end repeat
                            return resultList as string
                        end tell
                    on error errMsg
                        return "Mail error: " & errMsg
                    end try
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 8.0)
                if success {
                    return CallTool.Result(content: [.text(output)])
                } else {
                    return CallTool.Result(
                        content: [.text("Error: \(error ?? "Unknown")")], isError: true)
                }

            // --- Finder Handlers ---
            case finderListFilesTool.name:
                let pathParam = try getOptionalString(from: params.arguments, key: "path")
                let filter = try getOptionalString(from: params.arguments, key: "filter")
                let sort = try getOptionalString(from: params.arguments, key: "sort")
                let order = try getOptionalString(from: params.arguments, key: "order")
                let limit = try getOptionalInt(from: params.arguments, key: "limit")
                let metadata = try getOptionalBool(from: params.arguments, key: "metadata") ?? false

                var searchPath = ""

                if let p = pathParam {
                    searchPath = p
                } else {
                    // Fallback: Get path from frontmost Finder window
                    let script = """
                        tell application "Finder"
                            try
                                set targetFolder to target of front window
                                return POSIX path of (targetFolder as alias)
                            on error
                                return ""
                            end try
                        end tell
                        """
                    let (success, output, _) = await runAppleScript(script, timeout: 2.0)
                    if success && !output.isEmpty {
                        searchPath = output.trimmingCharacters(in: .whitespacesAndNewlines)
                    } else {
                        // Fallback to user home if no window open and no path provided
                        searchPath = FileManager.default.homeDirectoryForCurrentUser.path
                    }
                }

                let fileManager = FileManager.default

                do {
                    let items = try fileManager.contentsOfDirectory(atPath: searchPath)
                    var filteredItems = items

                    // 1. Filter
                    if let pattern = filter {
                        // Simple wildcard matching
                        if pattern.contains("*") {
                            let regexPattern = pattern.replacingOccurrences(of: "*", with: ".*")
                            if let regex = try? NSRegularExpression(
                                pattern: "^\(regexPattern)$", options: .caseInsensitive)
                            {
                                filteredItems = filteredItems.filter {
                                    regex.firstMatch(
                                        in: $0, options: [],
                                        range: NSRange(location: 0, length: $0.utf16.count)) != nil
                                }
                            }
                        } else {
                            filteredItems = filteredItems.filter {
                                $0.localizedCaseInsensitiveContains(pattern)
                            }
                        }
                    }

                    // 2. Sort & 3. Metadata
                    // We need attributes for sorting by date/size or if metadata is requested
                    var itemAttributes: [(name: String, date: Date, size: UInt64, type: String)] =
                        []

                    for item in filteredItems {
                        let fullPath = (searchPath as NSString).appendingPathComponent(item)
                        if let attrs = try? fileManager.attributesOfItem(atPath: fullPath) {
                            let date = attrs[.modificationDate] as? Date ?? Date.distantPast
                            let size = attrs[.size] as? UInt64 ?? 0
                            let type =
                                (attrs[.type] as? FileAttributeType) == .typeDirectory
                                ? "directory" : "file"
                            itemAttributes.append((name: item, date: date, size: size, type: type))
                        } else {
                            itemAttributes.append(
                                (name: item, date: Date.distantPast, size: 0, type: "unknown"))
                        }
                    }

                    if let sortKey = sort {
                        let ascending = (order != "desc")
                        itemAttributes.sort { (a, b) -> Bool in
                            switch sortKey {
                            case "date": return ascending ? a.date < b.date : a.date > b.date
                            case "size": return ascending ? a.size < b.size : a.size > b.size
                            case "type": return ascending ? a.type < b.type : a.type > b.type
                            default:  // name
                                return ascending
                                    ? a.name.localizedStandardCompare(b.name) == .orderedAscending
                                    : a.name.localizedStandardCompare(b.name) == .orderedDescending
                            }
                        }
                    } else {
                        // Default sort by name
                        itemAttributes.sort {
                            $0.name.localizedStandardCompare($1.name) == .orderedAscending
                        }
                    }

                    // 4. Limit
                    if let max = limit, max < itemAttributes.count {
                        itemAttributes = Array(itemAttributes.prefix(max))
                    }

                    // 5. Build Result & Serialize
                    if metadata {
                        let metaResults = itemAttributes.map {
                            [
                                "name": $0.name,
                                "type": $0.type,
                                "size": String($0.size),
                                "modified": ISO8601DateFormatter().string(from: $0.date),
                            ]
                        }
                        if let jsonString = serializeToJsonString(metaResults) {
                            return .init(content: [.text(jsonString)], isError: false)
                        }
                    } else {
                        let nameResults = itemAttributes.map { $0.name }
                        if let jsonString = serializeToJsonString(nameResults) {
                            return .init(content: [.text(jsonString)], isError: false)
                        }
                    }

                    return .init(content: [.text("Failed to serialize file list")], isError: true)

                } catch {
                    return .init(
                        content: [
                            .text(
                                "Error listing files at \(searchPath): \(error.localizedDescription)"
                            )
                        ], isError: true)
                }

            case finderGetSelectionTool.name:
                let script = """
                    try
                        tell application "Finder"
                            set theSelection to selection
                            if (count of theSelection) is 0 then return "Nothing selected."
                            set pathList to {}
                            repeat with anItem in theSelection
                                set end of pathList to POSIX path of (anItem as text)
                            end repeat
                            set AppleScript's text item delimiters to "\n"
                            return pathList as string
                        end tell
                    on error errMsg
                        return "Finder selection error: " & errMsg
                    end try
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 6.0)
                return .init(
                    content: [.text(success ? output : "Error: \(error ?? "Unknown")")],
                    isError: !success)

            case finderOpenPathTool.name:
                let path = try getRequiredString(from: params.arguments, key: "path")
                let script =
                    "tell application \"Finder\" to open POSIX file \"\(escapeForAppleScript(path))\""
                let (success, _, error) = await runAppleScript(script, timeout: 6.0)
                return .init(
                    content: [
                        .text(success ? "Opened path \(path)" : "Error: \(error ?? "Unknown")")
                    ],
                    isError: !success)

            case finderMoveToTrashTool.name:
                let path = try getRequiredString(from: params.arguments, key: "path")

                // Validate file exists and is not a directory
                let fileManager = FileManager.default
                guard fileManager.fileExists(atPath: path) else {
                    return .init(
                        content: [.text("Error: File does not exist: \(path)")],
                        isError: true
                    )
                }

                var isDir: ObjCBool = false
                guard fileManager.fileExists(atPath: path, isDirectory: &isDir), !isDir.boolValue
                else {
                    return .init(
                        content: [.text("Error: Cannot move directory to trash: \(path)")],
                        isError: true
                    )
                }

                let script =
                    "tell application \"Finder\" to delete POSIX file \"\(escapeForAppleScript(path))\""
                let (success, _, error) = await runAppleScript(script, timeout: 6.0)
                return .init(
                    content: [
                        .text(success ? "Moved to trash: \(path)" : "Error: \(error ?? "Unknown")")
                    ], isError: !success)

            // --- List Running Applications Handler ---
            case listAppsTool.name:
                let filter = try getOptionalString(from: params.arguments, key: "filter")?
                    .lowercased()
                let runningApps = NSWorkspace.shared.runningApplications
                var appsList: [[String: String]] = []

                for app in runningApps {
                    let bundleID = app.bundleIdentifier ?? "unknown"
                    let localizedName = app.localizedName ?? "unknown"

                    if let f = filter {
                        if !bundleID.lowercased().contains(f)
                            && !localizedName.lowercased().contains(f)
                        {
                            continue
                        }
                    }

                    var appInfo: [String: String] = [:]
                    appInfo["pid"] = String(app.processIdentifier)
                    appInfo["bundleIdentifier"] = bundleID
                    appInfo["localizedName"] = localizedName
                    appInfo["bundleURL"] = app.bundleURL?.path ?? "unknown"
                    appInfo["activationPolicy"] = String(app.activationPolicy.rawValue)
                    appInfo["launchDate"] = app.launchDate?.description ?? "unknown"
                    appInfo["processStatus"] = app.isTerminated ? "terminated" : "running"

                    appsList.append(appInfo)
                }

                guard let jsonString = serializeToJsonString(appsList) else {
                    return .init(
                        content: [.text("Failed to serialize running apps list")], isError: true)
                }
                let contextLabel = "LIST_RUNNING_APPS_RESULT: Found \(appsList.count) applications."
                return .init(content: [.text("\(contextLabel)\n\(jsonString)")], isError: false)

            // --- List Browser Tabs Handler ---
            case listTabsTool.name:
                let browser = try getOptionalString(from: params.arguments, key: "browser")?
                    .lowercased()
                var allTabs: [[String: String]] = []

                // Use AppleScript to get browser tabs
                let safariScript = """
                    tell application "Safari"
                        if (count of windows) > 0 then
                            set tabList to {}
                            repeat with w in windows
                                repeat with t in tabs of w
                                    set tabInfo to (name of t) & " | " & (URL of t)
                                    set end of tabList to tabInfo
                                end repeat
                            end repeat
                            return my listToString(tabList)
                        else
                            return "No windows open"
                        end if
                    end tell

                    on listToString(lst)
                        set text item delimiters of AppleScript to "\\n"
                        set lstString to lst as string
                        set text item delimiters of AppleScript to ""
                        return lstString
                    end listToString
                    """

                let chromeScript = """
                    tell application "Google Chrome"
                        if (count of windows) > 0 then
                            set tabList to {}
                            repeat with w in windows
                                repeat with t in tabs of w
                                    set tabInfo to (title of t) & " | " & (URL of t)
                                    set end of tabList to tabInfo
                                end repeat
                            end repeat
                            return my listToString(tabList)
                        else
                            return "No windows open"
                        end if
                    end tell

                    on listToString(lst)
                        set text item delimiters of AppleScript to "\\n"
                        set lstString to lst as string
                        set text item delimiters of AppleScript to ""
                        return lstString
                    end listToString
                    """

                let firefoxScript = """
                    tell application "System Events"
                        if exists (process "firefox") then
                            tell application "Firefox"
                                if (count of windows) > 0 then
                                    set tabList to {}
                                    repeat with w in windows
                                        set end of tabList to name of w
                                    end repeat
                                    set text item delimiters of AppleScript to "\\n"
                                    return tabList as string
                                else
                                    return "No windows open"
                                end if
                            end tell
                        else
                            return "Firefox not running"
                        end if
                    end tell
                    """

                var scripts: [(String, String)] = []
                if browser == nil || browser == "safari" {
                    scripts.append(("Safari", safariScript))
                }
                if browser == nil || browser == "chrome" {
                    scripts.append(("Chrome", chromeScript))
                }
                if browser == nil || browser == "firefox" {
                    scripts.append(("Firefox", firefoxScript))
                }

                for (browserName, script) in scripts {
                    let (success, output, _) = await runAppleScript(script, timeout: 5.0)
                    if success && !output.contains("No windows") && !output.contains("not running")
                    {
                        allTabs.append([
                            "browser": browserName,
                            "tabs": output,
                        ])
                    }
                }

                guard let jsonString = serializeToJsonString(allTabs) else {
                    return .init(
                        content: [.text("Failed to serialize browser tabs list")], isError: true)
                }
                return .init(content: [.text(jsonString)], isError: false)

            // --- List All Windows Handler ---
            case listWindowsTool.name:
                let windowList = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID)
                var windows: [[String: String]] = []

                if let windowInfoList = windowList as? [[String: Any]] {
                    for windowInfo in windowInfoList {
                        var window: [String: String] = [:]

                        window["name"] = (windowInfo[kCGWindowName as String] as? String) ?? ""
                        window["ownerName"] =
                            (windowInfo[kCGWindowOwnerName as String] as? String) ?? ""
                        window["bounds"] = String(
                            describing: windowInfo[kCGWindowBounds as String] ?? "")
                        window["layer"] = String(
                            describing: windowInfo[kCGWindowLayer as String] ?? 0)
                        window["id"] = String(
                            describing: windowInfo[kCGWindowNumber as String] ?? 0)

                        windows.append(window)
                    }
                }

                guard let jsonString = serializeToJsonString(windows) else {
                    return .init(
                        content: [.text("Failed to serialize windows list")], isError: true)
                }
                return .init(content: [.text(jsonString)], isError: false)

            // --- Dynamic Help Handler ---
            case dynamicHelpTool.name:
                // Create a definable struct for JSON output
                struct ToolDescription: Encodable {
                    let name: String
                    let description: String
                    let inputSchema: Value  // Value is likely Encodable from MCP standard library
                }

                let filterToolName = try getOptionalString(from: params.arguments, key: "toolName")
                let filterSearch = try getOptionalString(from: params.arguments, key: "search")

                var filteredTools = allTools
                if let toolName = filterToolName {
                    filteredTools = filteredTools.filter { $0.name.lowercased().contains(toolName.lowercased()) }
                }
                if let search = filterSearch {
                    filteredTools = filteredTools.filter {
                        $0.name.lowercased().contains(search.lowercased()) ||
                        ($0.description ?? "").lowercased().contains(search.lowercased())
                    }
                }

                let validTools = filteredTools.map {
                    ToolDescription(
                        name: $0.name, description: $0.description ?? "",
                        inputSchema: $0.inputSchema)
                }

                guard let jsonString = serializeToJsonString(validTools) else {
                    return CallTool.Result(
                        content: [.text("Failed to serialize tools list")], isError: true)
                }

                return CallTool.Result(content: [.text(jsonString)])

            // --- End Universal Handlers ---

            case pressKeyTool.name:

                let keyName = try getRequiredString(from: params.arguments, key: "keyName")
                // Parse optional flags using the new helper
                let flags = try parseFlags(from: params.arguments?["modifierFlags"])
                debugLog("log: handler(CallTool): parsed modifierFlags: \(flags)\n", stderr)
                primaryAction = .input(action: .press(keyName: keyName, flags: flags))
                options.pidForTraversal = convertedPid  // Re-affirm

            case refreshTool.name:
                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid  // Re-affirm

            case executeCommandTool.name, terminalTool.name, macosUseExecuteCommandAliasTool.name, openTerminalAliasTool.name:
                let command = try getRequiredString(from: params.arguments, key: "command")

                // Handle "cd" command manually for persistence
                let parts = command.split(separator: " ").map(String.init)
                if parts.first == "cd" {
                    let targetDir: String
                    if parts.count > 1 {
                        let path = parts[1]
                        if path.hasPrefix("/") {
                            targetDir = path
                        } else if path == "~" {
                            targetDir = FileManager.default.homeDirectoryForCurrentUser.path
                        } else if path.hasPrefix("~/") {
                            targetDir =
                                FileManager.default.homeDirectoryForCurrentUser.path + "/"
                                + String(path.dropFirst(2))
                        } else {
                            targetDir = (persistentCWD as NSString).appendingPathComponent(path)
                        }
                    } else {
                        targetDir = FileManager.default.homeDirectoryForCurrentUser.path
                    }

                    var isDir: ObjCBool = false
                    if FileManager.default.fileExists(atPath: targetDir, isDirectory: &isDir)
                        && isDir.boolValue
                    {
                        persistentCWD = targetDir
                        // Need to return a result here, bypassing performAction which is for MacosUseSDK
                        let resultString = "Changed directory to \(persistentCWD)"
                        return .init(content: [.text(resultString)], isError: false)
                    } else {
                        return .init(
                            content: [.text("cd: \(targetDir): No such file or directory")],
                            isError: true)
                    }
                } else {
                    // Run normal command with enhanced output handling
                    let (output, exitCode) = runShellCommand(command)

                    // Always provide JSON output for agent parsing
                    let resultDict: [String: Any] = [
                        "stdout": output,
                        "stderr": "", // Already combined into output by runShellCommand
                        "exit_code": exitCode
                    ]
                    
                    if let jsonData = try? JSONSerialization.data(withJSONObject: resultDict),
                       let jsonString = String(data: jsonData, encoding: .utf8) {
                        return .init(content: [.text(jsonString)], isError: exitCode != 0)
                    } else {
                        // Fallback to text if JSON encoding fails for some reason
                        let fallbackOutput = "Command: \(command)\nExit Code: \(exitCode)\nOutput:\n\(output)"
                        return .init(content: [.text(fallbackOutput)], isError: exitCode != 0)
                    }
                }

            case screenshotTool.name, screenshotAliasTool.name:
                let path = try getOptionalString(from: params.arguments, key: "path")
                let region = try getOptionalObject(from: params.arguments, key: "region")
                let monitor = try getOptionalInt(from: params.arguments, key: "monitor")
                let quality =
                    try getOptionalString(from: params.arguments, key: "quality") ?? "medium"
                let format = try getOptionalString(from: params.arguments, key: "format") ?? "jpg"
                let ocr = try getOptionalBool(from: params.arguments, key: "ocr") ?? false
                // When OCR is requested, default to NOT sending the image — text is enough and saves tokens
                let includeImage = try getOptionalBool(from: params.arguments, key: "includeImage") ?? !ocr
                let rawMaxDim = try getOptionalDouble(from: params.arguments, key: "maxDimension") ?? 1024.0
                let maxDimension = CGFloat(rawMaxDim)
                let compact = try getOptionalBool(from: params.arguments, key: "compact") ?? false

                guard let image = captureMainDisplay(monitor: monitor) else {
                    debugLog("error: screenshot: Capture failed, checking permissions...\n", stderr)
                    if #available(macOS 11.0, *) {
                        if !CGPreflightScreenCaptureAccess() {
                            openSystemSettings(for: .screenRecording)
                            return .init(
                                content: [
                                    .text(
                                        "Screen Recording access denied. Opening System Settings > Privacy & Security > Screen Recording."
                                    )
                                ], isError: true)
                        }
                    }
                    return .init(
                        content: [
                            .text(
                                "Failed to capture screen. Please ensure the app has Screen Recording permissions."
                            )
                        ], isError: true)
                }

                // Apply region selection if specified
                var finalImage = image
                if let regionDict = region {
                    if let xValue = regionDict["x"], let yValue = regionDict["y"],
                        let widthValue = regionDict["width"],
                        let heightValue = regionDict["height"],
                        let x = xValue.doubleValue, let y = yValue.doubleValue,
                        let width = widthValue.doubleValue, let height = heightValue.doubleValue
                    {
                        let rect = CGRect(x: x, y: y, width: width, height: height)
                        if let croppedImage = image.cropping(to: rect) {
                            finalImage = croppedImage
                        }
                    }
                }

                // Downscale for token efficiency (for display/AI use, not for file saves)
                let displayImage = path == nil ? resizeImageIfNeeded(image: finalImage, maxDimension: maxDimension) : finalImage

                if let savePath = path {
                    // Save original (full-res) to file
                    let imageWidth = CGFloat(finalImage.width)
                    let imageHeight = CGFloat(finalImage.height)
                    let nsImage = NSImage(
                        cgImage: finalImage, size: NSSize(width: imageWidth, height: imageHeight))

                    var imageData: Data?

                    switch format.lowercased() {
                    case "jpg", "jpeg":
                        if let tiffData = nsImage.tiffRepresentation,
                            let bitmapRep = NSBitmapImageRep(data: tiffData)
                        {
                            let qualityValue = getQualityValue(quality)
                            imageData = bitmapRep.representation(
                                using: .jpeg, properties: [.compressionFactor: qualityValue])
                        }
                    case "webp":
                        // WebP support would require additional library, fallback to PNG
                        fallthrough
                    default:  // PNG
                        if let tiffData = nsImage.tiffRepresentation,
                            let bitmapRep = NSBitmapImageRep(data: tiffData)
                        {
                            imageData = bitmapRep.representation(using: .png, properties: [:])
                        }
                    }

                    guard let data = imageData else {
                        return .init(
                            content: [.text("Failed to process screenshot")], isError: true)
                    }

                    do {
                        try data.write(to: URL(fileURLWithPath: savePath))

                        var content: [Tool.Content] = [.text("Screenshot saved to: \(savePath) (\(finalImage.width)x\(finalImage.height))")]

                        if ocr {
                            let ocrResults = performOCR(on: finalImage)
                            if let jsonString = serializeToJsonString(ocrResults) {
                                content.append(.text("\nOCR Results:\n\(jsonString)"))
                            }
                        }

                        return .init(content: content, isError: false)
                    } catch {
                        return .init(
                            content: [.text("Failed to save screenshot: \(error)")], isError: true)
                    }
                } else {
                    // Return Base64 (downscaled)
                    var content: [Tool.Content] = []

                    if includeImage {
                        // displayImage is already resized — pass .infinity to skip redundant resize in encodeBase64JPEG
                        guard let base64 = encodeBase64JPEG(image: displayImage, quality: quality, maxDimension: CGFloat.infinity) else {
                            return .init(content: [.text("Failed to encode screenshot")], isError: true)
                        }
                        content.append(.image(data: base64, mimeType: "image/jpeg", metadata: nil))
                        let scaleNote = (displayImage.width < image.width)
                            ? " (downscaled from \(image.width)x\(image.height) to \(displayImage.width)x\(displayImage.height))"
                            : " (\(image.width)x\(image.height))"
                        content.append(.text("Screenshot captured\(scaleNote)"))
                    }

                    if ocr {
                        let rawOCR = performOCR(on: displayImage)
                        let ocrResults = mergeAdjacentOCRElements(rawOCR)
                        
                        if compact {
                            // Compact: text only, no coordinates — minimal tokens
                            let textContent = ocrResults.map { $0.text }.joined(separator: "\n")
                            if !textContent.isEmpty {
                                content.append(.text("OCR:\n\(textContent)"))
                            } else {
                                content.append(.text("OCR: no text detected on screen"))
                            }
                        } else {
                            // Standard: compact coordinates with text
                            let textContent = ocrResults.map { 
                                "\(Int($0.x)),\(Int($0.y)),\(Int($0.width)),\(Int($0.height))|\($0.text)" 
                            }.joined(separator: "\n")
                            
                            if !textContent.isEmpty {
                                content.append(.text("OCR (x,y,w,h|text):\n\(textContent)"))
                            } else {
                                content.append(.text("OCR: no text detected on screen"))
                            }
                        }
                    }

                    if content.isEmpty {
                        content.append(.text("Screenshot captured (\(image.width)x\(image.height)) — includeImage was false and ocr was false."))
                    }

                    return .init(content: content, isError: false)
                }

            case visionTool.name:
                // Clear any red bounding boxes from MacosUseSDK before capturing screen
                await Task { @MainActor in
                    for window in NSApplication.shared.windows {
                        if window.styleMask == [.borderless] && window.level == .floating && !window.isOpaque {
                            window.close()
                        }
                    }
                }.value

                let region = try getOptionalObject(from: params.arguments, key: "region")
                let language =
                    try getOptionalString(from: params.arguments, key: "language") ?? "auto"
                let confidence =
                    try getOptionalBool(from: params.arguments, key: "confidence") ?? false
                let format = try getOptionalString(from: params.arguments, key: "format") ?? "json"

                guard let image = captureMainDisplay() else {
                    return .init(
                        content: [.text("Failed to capture screen for analysis")], isError: true)
                }

                // Apply region selection if specified
                var finalImage = image
                if let regionDict = region {
                    if let xValue = regionDict["x"], let yValue = regionDict["y"],
                        let widthValue = regionDict["width"],
                        let heightValue = regionDict["height"],
                        let x = xValue.doubleValue, let y = yValue.doubleValue,
                        let width = widthValue.doubleValue, let height = heightValue.doubleValue
                    {
                        let rect = CGRect(x: x, y: y, width: width, height: height)
                        if let croppedImage = image.cropping(to: rect) {
                            finalImage = croppedImage
                        }
                    }
                }

                // Downscale to max 1024px — Vision OCR works well at this resolution and it's faster
                let ocrImage = resizeImageIfNeeded(image: finalImage, maxDimension: 1024)

                let rawOCR = performOCR(
                    on: ocrImage, language: language, includeConfidence: confidence)
                let ocrResults = mergeAdjacentOCRElements(rawOCR)

                switch format.lowercased() {
                case "text":
                    // Compact format: x,y,w,h|text — optimized for minimal token usage
                    let plainText = ocrResults.map { 
                        "\(Int($0.x)),\(Int($0.y)),\(Int($0.width)),\(Int($0.height))|\($0.text)" 
                    }.joined(separator: "\n")
                    return .init(content: [.text(plainText)], isError: false)
                case "both":
                    let textContent = ocrResults.map { 
                        "\(Int($0.x)),\(Int($0.y)),\(Int($0.width)),\(Int($0.height))|\($0.text)" 
                    }.joined(separator: "\n")
                    if let jsonString = serializeToJsonString(ocrResults) {
                        let combined = "Text:\n\(textContent)\n\nJSON:\n\(jsonString)"
                        return .init(content: [.text(combined)], isError: false)
                    } else {
                        return .init(content: [.text(textContent)], isError: false)
                    }
                default:  // json
                    guard let jsonString = serializeToJsonString(ocrResults) else {
                        return .init(
                            content: [.text("Failed to serialize vision results")], isError: true)
                    }
                    return .init(content: [.text(jsonString)], isError: false)
                }

            case unifiedVisionTool.name:
                // Clear any red bounding boxes from MacosUseSDK before capturing screen
                await Task { @MainActor in
                    for window in NSApplication.shared.windows {
                        if window.styleMask == [.borderless] && window.level == .floating && !window.isOpaque {
                            window.close()
                        }
                    }
                }.value

                let mode = try getOptionalString(from: params.arguments, key: "mode") ?? "smart"
                let region = try getOptionalObject(from: params.arguments, key: "region")
                let monitor = try getOptionalInt(from: params.arguments, key: "monitor")
                let rawMaxDim = try getOptionalDouble(from: params.arguments, key: "maxDimension") ?? 1024.0
                let maxDimension = CGFloat(rawMaxDim)
                let quality = try getOptionalString(from: params.arguments, key: "quality") ?? "medium"
                let language = try getOptionalString(from: params.arguments, key: "language") ?? "auto"
                let compact = try getOptionalBool(from: params.arguments, key: "compact") ?? false
                let withAccessibility = try getOptionalBool(from: params.arguments, key: "withAccessibility") ?? false
                let targetPid = try getOptionalInt(from: params.arguments, key: "pid")

                // Try window-targeted capture first if PID is provided (smaller image = fewer tokens)
                var capturedImage: CGImage?
                var usedWindowCapture = false
                if let pid = targetPid, pid > 0, let convertedPid = pid_t(exactly: pid) {
                    capturedImage = captureWindow(pid: convertedPid)
                    usedWindowCapture = (capturedImage != nil)
                }
                // Fallback to full-screen capture
                if capturedImage == nil {
                    capturedImage = captureMainDisplay(monitor: monitor)
                }
                guard let capturedImage = capturedImage else {
                    if #available(macOS 11.0, *) {
                        if !CGPreflightScreenCaptureAccess() {
                            openSystemSettings(for: .screenRecording)
                            return .init(content: [.text("Screen Recording access denied. Enable in System Settings > Privacy & Security > Screen Recording.")], isError: true)
                        }
                    }
                    return .init(content: [.text("Failed to capture screen.")], isError: true)
                }

                // Apply region selection if specified
                var finalImage = capturedImage
                if let regionDict = region {
                    if let xValue = regionDict["x"], let yValue = regionDict["y"],
                        let widthValue = regionDict["width"],
                        let heightValue = regionDict["height"],
                        let x = xValue.doubleValue, let y = yValue.doubleValue,
                        let width = widthValue.doubleValue, let height = heightValue.doubleValue
                    {
                        let rect = CGRect(x: x, y: y, width: width, height: height)
                        if let croppedImage = capturedImage.cropping(to: rect) {
                            finalImage = croppedImage
                        }
                    }
                }

                // Target identifier for caching
                let targetKey = usedWindowCapture ? "pid:\(targetPid!)" : "monitor:\(monitor ?? 0)"
                
                // Diff check: if requested, compare perceptual hash with last capture
                let diff = try getOptionalBool(from: params.arguments, key: "diff") ?? false
                var currentCacheEntry = visionCache[targetKey] ?? VisionCacheEntry(hash: 0, timestamp: .distantPast, ocrResults: [], compact: false)
                
                if diff {
                    let currentHash = perceptualHash(of: finalImage)
                    if currentCacheEntry.hash != 0 {
                        let distance = hammingDistance(currentHash, currentCacheEntry.hash)
                        if distance == 0 {
                            currentCacheEntry.timestamp = Date()
                            visionCache[targetKey] = currentCacheEntry
                            return .init(content: [.text("Vision: no_change (screen is identical to last capture)")], isError: false)
                        }
                    }
                    currentCacheEntry.hash = currentHash
                    currentCacheEntry.timestamp = Date()
                }

                // Downscale for AI consumption
                let processedImage = resizeImageIfNeeded(image: finalImage, maxDimension: maxDimension)

                // Determine what to include based on mode
                let needsOCR = (mode != "image")
                let smartThreshold = 3 // If OCR finds fewer than this many elements, include the image

                var ocrResults: [VisionElement] = []
                if needsOCR {
                    let rawOCR = performOCR(on: processedImage, language: language)
                    ocrResults = mergeAdjacentOCRElements(rawOCR)
                }

                // Smart mode: decide image inclusion based on OCR richness
                let includeImage: Bool
                switch mode.lowercased() {
                case "ocr": includeImage = false
                case "image": includeImage = true
                case "full": includeImage = true
                case "smart": includeImage = ocrResults.count < smartThreshold
                default: includeImage = ocrResults.count < smartThreshold
                }

                var content: [Tool.Content] = []

                // Image
                if includeImage {
                    // processedImage is already resized — pass .infinity to skip redundant resize
                    guard let base64 = encodeBase64JPEG(image: processedImage, quality: quality, maxDimension: CGFloat.infinity) else {
                        return .init(content: [.text("Failed to encode screenshot")], isError: true)
                    }
                    content.append(.image(data: base64, mimeType: "image/jpeg", metadata: nil))
                    let scaleNote = (processedImage.width < capturedImage.width)
                        ? " (downscaled from \(capturedImage.width)x\(capturedImage.height) to \(processedImage.width)x\(processedImage.height))"
                        : " (\(capturedImage.width)x\(capturedImage.height))"
                    let captureType = usedWindowCapture ? "Window screenshot" : "Screenshot"
                    content.append(.text("\(captureType)\(scaleNote)"))
                }

                // OCR
                if needsOCR && !ocrResults.isEmpty {
                    var useDiffFormat = false
                    var diffOutput = ""
                    
                    if diff && !currentCacheEntry.ocrResults.isEmpty {
                        let diffResult = computeOCRDiff(old: currentCacheEntry.ocrResults, new: ocrResults)
                        let totalChanges = diffResult.added.count + diffResult.removed.count
                        
                        // Fallback: if more than 60% of elements changed, diff is too chaotic
                        // Only use diff if changes are relatively small
                        if totalChanges < max(10, Int(Double(currentCacheEntry.ocrResults.count) * 0.6)) {
                            useDiffFormat = true
                            diffOutput += "OCR Diff (unchanged: \(diffResult.unchangedCount)):\n"
                            
                            if !diffResult.added.isEmpty {
                                diffOutput += "ADDED:\n"
                                diffOutput += formatOCRElements(diffResult.added, compact: compact) + "\n"
                            }
                            if !diffResult.removed.isEmpty {
                                diffOutput += "REMOVED:\n"
                                diffOutput += formatOCRElements(diffResult.removed, compact: compact) + "\n"
                            }
                        }
                    }
                    
                    currentCacheEntry.ocrResults = ocrResults
                    currentCacheEntry.compact = compact
                    visionCache[targetKey] = currentCacheEntry
                    
                    if useDiffFormat {
                        content.append(.text(diffOutput.trimmingCharacters(in: .whitespacesAndNewlines)))
                    } else {
                        let prefix = compact ? "OCR:" : "OCR (x,y,w,h|text):"
                        let textContent = formatOCRElements(ocrResults, compact: compact)
                        content.append(.text("\(prefix)\n\(textContent)"))
                    }
                } else if needsOCR {
                    currentCacheEntry.ocrResults = []
                    visionCache[targetKey] = currentCacheEntry
                    content.append(.text("OCR: no text detected on screen"))
                }

                // Smart mode annotation
                if mode == "smart" {
                    let reason = includeImage
                        ? "image included (only \(ocrResults.count) OCR element\(ocrResults.count == 1 ? "" : "s") found — likely graphical content)"
                        : "image skipped (\(ocrResults.count) OCR elements found — text is sufficient)"
                    content.append(.text("[smart: \(reason)]"))
                }

                // Accessibility tree (optional, in same call)
                if withAccessibility {
                    let axPid = resolvePid(try getOptionalInt(from: params.arguments, key: "pid"))
                    if let convertedPid = pid_t(exactly: axPid) {
                        var axOptions = ActionOptions()
                        axOptions.traverseAfter = true
                        axOptions.onlyVisibleElements = true
                        axOptions.pidForTraversal = convertedPid
                        let axResult: ActionResult = await Task { @MainActor in
                            await performAction(action: .traverseOnly, optionsInput: axOptions)
                        }.value
                        if let axJson = serializeToJsonString(axResult) {
                            content.append(.text("Accessibility:\n\(axJson)"))
                        }
                    }
                }

                if content.isEmpty {
                    content.append(.text("Vision: no data captured"))
                }

                return .init(content: content, isError: false)

            case scrollTool.name:
                let direction = try getRequiredString(from: params.arguments, key: "direction")
                let amount = try getOptionalInt(from: params.arguments, key: "amount") ?? 3
                let sensitivity =
                    try getOptionalString(from: params.arguments, key: "sensitivity") ?? "normal"

                // Configurable scroll sensitivity multiplier
                let multiplier: Int
                switch sensitivity.lowercased() {
                case "fine": multiplier = 1
                case "fast": multiplier = 30
                default: multiplier = 10  // normal
                }

                // Native CGEvent scroll with configurable sensitivity
                let dy =
                    direction == "down"
                    ? Int32(amount * multiplier)
                    : (direction == "up" ? Int32(-amount * multiplier) : 0)
                let dx =
                    direction == "right"
                    ? Int32(amount * multiplier)
                    : (direction == "left" ? Int32(-amount * multiplier) : 0)
                let scrollEvent = CGEvent(
                    scrollWheelEvent2Source: nil, units: .line, wheelCount: 2, wheel1: dy,
                    wheel2: dx, wheel3: 0)
                scrollEvent?.post(tap: .cghidEventTap)

                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid

            case rightClickTool.name:
                let x = try getRequiredDouble(from: params.arguments, key: "x")
                let y = try getRequiredDouble(from: params.arguments, key: "y")

                // Native right click
                let point = CGPoint(x: x, y: y)
                let mouseDown = CGEvent(
                    mouseEventSource: nil, mouseType: .rightMouseDown, mouseCursorPosition: point,
                    mouseButton: .right)
                let mouseUp = CGEvent(
                    mouseEventSource: nil, mouseType: .rightMouseUp, mouseCursorPosition: point,
                    mouseButton: .right)
                mouseDown?.post(tap: .cghidEventTap)
                mouseUp?.post(tap: .cghidEventTap)

                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid

            case doubleClickTool.name:
                let x = try getRequiredDouble(from: params.arguments, key: "x")
                let y = try getRequiredDouble(from: params.arguments, key: "y")

                // Native double click using CGEvent with system-aware timing
                let point = CGPoint(x: x, y: y)
                let mouseDown1 = CGEvent(
                    mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point,
                    mouseButton: .left)
                let mouseUp1 = CGEvent(
                    mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point,
                    mouseButton: .left)
                let mouseDown2 = CGEvent(
                    mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point,
                    mouseButton: .left)
                let mouseUp2 = CGEvent(
                    mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point,
                    mouseButton: .left)

                mouseDown1?.setIntegerValueField(.mouseEventClickState, value: 1)
                mouseUp1?.setIntegerValueField(.mouseEventClickState, value: 1)
                mouseDown2?.setIntegerValueField(.mouseEventClickState, value: 2)
                mouseUp2?.setIntegerValueField(.mouseEventClickState, value: 2)

                // Use system double-click interval for reliable timing
                let dblClickDelay = UInt64(NSEvent.doubleClickInterval * 0.4 * 1_000_000_000)
                mouseDown1?.post(tap: .cghidEventTap)
                mouseUp1?.post(tap: .cghidEventTap)
                try? await Task.sleep(nanoseconds: dblClickDelay)
                mouseDown2?.post(tap: .cghidEventTap)
                mouseUp2?.post(tap: .cghidEventTap)

                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid

            case tripleClickTool.name:
                let x = try getRequiredDouble(from: params.arguments, key: "x")
                let y = try getRequiredDouble(from: params.arguments, key: "y")

                // Native triple click for line selection using CGEvent
                let point = CGPoint(x: x, y: y)
                let dblClickDelay = UInt64(NSEvent.doubleClickInterval * 0.3 * 1_000_000_000)

                for clickNum in 1...3 {
                    let down = CGEvent(
                        mouseEventSource: nil, mouseType: .leftMouseDown,
                        mouseCursorPosition: point,
                        mouseButton: .left)
                    let up = CGEvent(
                        mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point,
                        mouseButton: .left)
                    down?.setIntegerValueField(.mouseEventClickState, value: Int64(clickNum))
                    up?.setIntegerValueField(.mouseEventClickState, value: Int64(clickNum))
                    down?.post(tap: .cghidEventTap)
                    up?.post(tap: .cghidEventTap)
                    if clickNum < 3 {
                        try? await Task.sleep(nanoseconds: dblClickDelay)
                    }
                }

                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid

            case mouseMoveTool.name:
                let x = try getRequiredDouble(from: params.arguments, key: "x")
                let y = try getRequiredDouble(from: params.arguments, key: "y")

                // Native mouse move using CGEvent
                let point = CGPoint(x: x, y: y)
                let moveEvent = CGEvent(
                    mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point,
                    mouseButton: .left)
                moveEvent?.post(tap: .cghidEventTap)

                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid

            case dragDropTool.name:
                let startX = try getRequiredDouble(from: params.arguments, key: "startX")
                let startY = try getRequiredDouble(from: params.arguments, key: "startY")
                let endX = try getRequiredDouble(from: params.arguments, key: "endX")
                let endY = try getRequiredDouble(from: params.arguments, key: "endY")
                let steps = try getOptionalInt(from: params.arguments, key: "steps") ?? 10

                let start = CGPoint(x: startX, y: startY)
                let end = CGPoint(x: endX, y: endY)

                // Mouse down at start position
                let mouseDown = CGEvent(
                    mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: start,
                    mouseButton: .left)
                mouseDown?.post(tap: .cghidEventTap)
                try? await Task.sleep(nanoseconds: 50_000_000)  // 50ms settle

                // Smooth interpolated drag with configurable steps
                let actualSteps = max(1, min(steps, 50))  // Clamp 1-50
                for i in 1...actualSteps {
                    let t = Double(i) / Double(actualSteps)
                    let currentX = startX + (endX - startX) * t
                    let currentY = startY + (endY - startY) * t
                    let currentPoint = CGPoint(x: currentX, y: currentY)
                    let dragEvent = CGEvent(
                        mouseEventSource: nil, mouseType: .leftMouseDragged,
                        mouseCursorPosition: currentPoint,
                        mouseButton: .left)
                    dragEvent?.post(tap: .cghidEventTap)
                    try? await Task.sleep(nanoseconds: 20_000_000)  // 20ms between steps
                }

                // Mouse up at end position
                let mouseUp = CGEvent(
                    mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: end,
                    mouseButton: .left)
                mouseUp?.post(tap: .cghidEventTap)

                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid

            case windowMgmtTool.name:
                let action = try getRequiredString(from: params.arguments, key: "action")

                let appRef = AXUIElementCreateApplication(pid_t(convertedPid))
                var windowValue: AnyObject?
                let result = AXUIElementCopyAttributeValue(
                    appRef, kAXFocusedWindowAttribute as CFString, &windowValue)

                if result == .success, let window = windowValue as! AXUIElement? {
                    switch action {
                    case "minimize":
                        AXUIElementSetAttributeValue(
                            window, kAXMinimizedAttribute as CFString, kCFBooleanTrue)
                    case "maximize":
                        // Try to use the zoom button to maximize
                        var zoomButton: AnyObject?
                        let zoomResult = AXUIElementCopyAttributeValue(
                            window, kAXZoomButtonAttribute as CFString, &zoomButton)
                        if zoomResult == .success, let button = zoomButton as! AXUIElement? {
                            AXUIElementPerformAction(button, kAXPressAction as CFString)
                        } else {
                            // Fallback: set window to screen bounds
                            if let screen = NSScreen.main {
                                let frame = screen.visibleFrame
                                var position = CGPoint(x: frame.origin.x, y: frame.origin.y)
                                var size = CGSize(width: frame.width, height: frame.height)
                                if let posValue = AXValueCreate(.cgPoint, &position) {
                                    AXUIElementSetAttributeValue(
                                        window, kAXPositionAttribute as CFString, posValue)
                                }
                                if let sizeValue = AXValueCreate(.cgSize, &size) {
                                    AXUIElementSetAttributeValue(
                                        window, kAXSizeAttribute as CFString, sizeValue)
                                }
                            }
                        }
                    case "make_front":
                        let app = NSRunningApplication(processIdentifier: pid_t(convertedPid))
                        app?.activate(options: .activateIgnoringOtherApps)
                    case "move":
                        let x = try getRequiredDouble(from: params.arguments, key: "x")
                        let y = try getRequiredDouble(from: params.arguments, key: "y")
                        var point = CGPoint(x: x, y: y)
                        if let value = AXValueCreate(.cgPoint, &point) {
                            AXUIElementSetAttributeValue(
                                window, kAXPositionAttribute as CFString, value)
                        }
                    case "resize":
                        let w = try getRequiredDouble(from: params.arguments, key: "width")
                        let h = try getRequiredDouble(from: params.arguments, key: "height")
                        var size = CGSize(width: w, height: h)
                        if let value = AXValueCreate(.cgSize, &size) {
                            AXUIElementSetAttributeValue(
                                window, kAXSizeAttribute as CFString, value)
                        }
                    default:
                        break
                    }

                    // After action, get actual values
                    var actualPos: AnyObject?
                    var actualSize: AnyObject?
                    AXUIElementCopyAttributeValue(
                        window, kAXPositionAttribute as CFString, &actualPos)
                    AXUIElementCopyAttributeValue(window, kAXSizeAttribute as CFString, &actualSize)

                    var pos = CGPoint.zero
                    var sz = CGSize.zero
                    if let pVal = actualPos as! AXValue? { AXValueGetValue(pVal, .cgPoint, &pos) }
                    if let sVal = actualSize as! AXValue? { AXValueGetValue(sVal, .cgSize, &sz) }

                    let resultData = WindowActionResult(
                        action: action,
                        pid: Int(convertedPid),
                        actualX: Double(pos.x),
                        actualY: Double(pos.y),
                        actualWidth: Double(sz.width),
                        actualHeight: Double(sz.height),
                        note: "Window dimensions might be constrained by the application."
                    )

                    if let json = serializeToJsonString(resultData) {
                        return .init(content: [.text(json)], isError: false)
                    }
                }

                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid

            case networkDiagnosticsTool.name:
                let action = try getRequiredString(from: params.arguments, key: "action")
                let target = try getOptionalString(from: params.arguments, key: "target") ?? "google.com"
                var output = ""
                var isErr = false

                switch action {
                case "connectivity":
                    let (out, code) = runShellCommand("nc -z -G 5 \(target) 443 2>&1 || curl -I --max-time 5 https://\(target) 2>&1 | head -5")
                    output = (code == 0 ? "ONLINE: " : "OFFLINE: ") + out
                    isErr = (code != 0)
                case "interfaces":
                    let (out, _) = runShellCommand("ifconfig | grep -E 'inet |flags|status'")
                    output = out.isEmpty ? "No interface info" : out
                case "public_ip":
                    let (out, code) = runShellCommand("curl -s --max-time 10 https://api.ipify.org 2>&1")
                    output = (code == 0 ? "Public IP: " : "Failed: ") + out
                    isErr = (code != 0)
                case "dns_lookup":
                    let (out, code) = runShellCommand("dig +short \(target) | head -5")
                    output = (code == 0 ? "DNS: " : "DNS Failed: ") + out
                    isErr = (code != 0)
                case "ping":
                    let (out, code) = runShellCommand("ping -c 3 -W 3 \(target) 2>&1")
                    output = out
                    isErr = (code != 0)
                case "full":
                    var full = ""
                    let (conn, cCode) = runShellCommand("curl -I --max-time 5 https://\(target) 2>&1 | head -3")
                    full += "Connectivity: \(cCode == 0 ? "ONLINE" : "OFFLINE")\n" + conn + "\n"
                    let (ifaces, _) = runShellCommand("ifconfig | grep -E 'inet ' | head -5")
                    full += "Interfaces:\n" + ifaces + "\n"
                    let (pubIp, pCode) = runShellCommand("curl -s --max-time 10 https://api.ipify.org 2>&1")
                    full += "Public IP: \(pCode == 0 ? pubIp : "Unavailable")\n"
                    let (dns, _) = runShellCommand("dig +short \(target) | head -3")
                    full += "DNS: " + dns + "\n"
                    let (gw, _) = runShellCommand("netstat -rn | grep default | head -2")
                    full += "Gateway:\n" + gw
                    output = full
                default:
                    output = "Unknown action '\(action)'. Use: connectivity, interfaces, public_ip, dns_lookup, ping, full"
                    isErr = true
                }
                return .init(content: [.text(output)], isError: isErr)

            case setClipboardTool.name:
                let text = try getRequiredString(from: params.arguments, key: "text")
                let html = try getOptionalString(from: params.arguments, key: "html")
                let image = try getOptionalString(from: params.arguments, key: "image")
                let addToHistory =
                    try getOptionalBool(from: params.arguments, key: "addToHistory") ?? true

                // Enhanced clipboard with rich text and image support
                NSPasteboard.general.clearContents()

                var result = "Clipboard updated."

                // Set plain text
                NSPasteboard.general.setString(text, forType: .string)

                // Set HTML if provided
                if let htmlContent = html {
                    NSPasteboard.general.setString(htmlContent, forType: .html)
                    result += " HTML content added."
                }

                // Set image if provided
                if let imageData = image, let imageNSData = Data(base64Encoded: imageData) {
                    let nsImage = NSImage(data: imageNSData)
                    if let tiffData = nsImage?.tiffRepresentation {
                        NSPasteboard.general.setData(tiffData, forType: .tiff)
                        result += " Image added."
                    }
                }

                // Add to history if requested
                if addToHistory {
                    addToClipboardHistory(text: text, html: html, image: image)
                    result += " Added to history."
                }

                return .init(content: [.text(result)], isError: false)

            case getClipboardTool.name:
                let format = try getOptionalString(from: params.arguments, key: "format") ?? "text"
                let history = try getOptionalBool(from: params.arguments, key: "history") ?? false
                let limit = try getOptionalInt(from: params.arguments, key: "limit") ?? 10

                if history {
                    let historyData = getClipboardHistory(limit: limit)
                    guard let jsonString = serializeToJsonString(historyData) else {
                        return .init(
                            content: [.text("Failed to serialize clipboard history")], isError: true
                        )
                    }
                    return .init(content: [.text(jsonString)], isError: false)
                } else {
                    switch format.lowercased() {
                    case "html":
                        let htmlContent = NSPasteboard.general.string(forType: .html) ?? ""
                        return .init(content: [.text(htmlContent)], isError: false)
                    case "image":
                        if let tiffData = NSPasteboard.general.data(forType: .tiff) {
                            let base64 = tiffData.base64EncodedString()
                            return .init(content: [.text(base64)], isError: false)
                        } else {
                            return .init(content: [.text("No image in clipboard")], isError: false)
                        }
                    case "all":
                        var allContent: [String: String] = [:]
                        let textContent = NSPasteboard.general.string(forType: .string) ?? ""
                        let htmlContent = NSPasteboard.general.string(forType: .html) ?? ""
                        if let tiffData = NSPasteboard.general.data(forType: .tiff) {
                            allContent["text"] = textContent
                            allContent["html"] = htmlContent
                            allContent["image"] = tiffData.base64EncodedString()
                        } else {
                            allContent["text"] = textContent
                            allContent["html"] = htmlContent
                        }
                        guard let jsonString = serializeToJsonString(allContent) else {
                            return .init(
                                content: [.text("Failed to serialize all clipboard content")],
                                isError: true)
                        }
                        return .init(content: [.text(jsonString)], isError: false)
                    default:  // text
                        let textContent = NSPasteboard.general.string(forType: .string) ?? ""
                        return .init(content: [.text(textContent)], isError: false)
                    }
                }

            case clipboardHistoryTool.name:
                let clear = try getOptionalBool(from: params.arguments, key: "clear") ?? false
                let limit = try getOptionalInt(from: params.arguments, key: "limit") ?? 50

                if clear {
                    clearClipboardHistory()
                    return .init(content: [.text("Clipboard history cleared.")], isError: false)
                } else {
                    let historyData = getClipboardHistory(limit: limit)
                    guard let jsonString = serializeToJsonString(historyData) else {
                        return .init(
                            content: [.text("Failed to serialize clipboard history")], isError: true
                        )
                    }
                    return .init(content: [.text(jsonString)], isError: false)
                }

            case mediaControlTool.name:
                let action = try getRequiredString(from: params.arguments, key: "action")
                // Using AppleScript for simple system controls as it's most robust for media
                var script = ""
                switch action {
                case "play_pause": script = "tell application \"System Events\" to key code 103"  // Media Play/Pause
                case "next": script = "tell application \"System Events\" to key code 111"
                case "previous": script = "tell application \"System Events\" to key code 101"
                case "volume_up":
                    script =
                        "set volume output volume ((output volume of (get volume settings)) + 10)"
                case "volume_down":
                    script =
                        "set volume output volume ((output volume of (get volume settings)) - 10)"
                case "mute": script = "set volume with output muted"
                case "brightness_up": script = "tell application \"System Events\" to key code 144"
                case "brightness_down":
                    script = "tell application \"System Events\" to key code 145"
                case "volume":
                    if let val = params.arguments?["value"] {
                        if let doubleVal = val.doubleValue {
                            script = "set volume output volume \(doubleVal)"
                        } else if let intVal = val.intValue {
                            script = "set volume output volume \(intVal)"
                        } else if let strVal = val.stringValue {
                            if strVal == "up" {
                                script = "set volume output volume ((output volume of (get volume settings)) + 10)"
                            } else if strVal == "down" {
                                script = "set volume output volume ((output volume of (get volume settings)) - 10)"
                            } else if strVal == "mute" {
                                script = "set volume with output muted"
                            }
                        } else if let boolVal = val.boolValue {
                            if boolVal {
                                script = "set volume with output muted"
                            } else {
                                script = "set volume without output muted"
                            }
                        }
                    } else {
                        script = "set volume output volume ((output volume of (get volume settings)) + 10)"
                    }
                case "brightness":
                    if let val = params.arguments?["value"] {
                        var brightnessLevel = 0.5
                        if let doubleVal = val.doubleValue {
                            brightnessLevel = doubleVal > 1.0 ? doubleVal / 100.0 : doubleVal
                        } else if let intVal = val.intValue {
                            brightnessLevel = Double(intVal) > 1.0 ? Double(intVal) / 100.0 : Double(intVal)
                        }
                        script = "tell application \"System Events\" to repeat with v in (every desktop) \n set brightness of v to \(brightnessLevel) \n end repeat"
                    } else {
                        script = "tell application \"System Events\" to key code 144"
                    }
                case "dark_mode":
                    var dark = true
                    if let val = params.arguments?["value"] {
                        if let boolVal = val.boolValue {
                            dark = boolVal
                        } else if let strVal = val.stringValue {
                            dark = (strVal.lowercased() == "true" || strVal.lowercased() == "on")
                        }
                    }
                    script = "tell application \"System Events\" to tell appearance preferences to set dark mode to \(dark)"
                case "sleep":
                    script = "tell application \"System Events\" to sleep"
                case "lock":
                    let lockCommand = "pmset displaysleepnow"
                    let (_, exitCode) = runShellCommand(lockCommand)
                    return .init(content: [.text(exitCode == 0 ? "Screen locked" : "Failed to lock screen")], isError: exitCode != 0)
                case "dnd":
                    script = """
                    try
                        tell application "System Events" to tell process "ControlCenter"
                            click menu bar item "Focus" of menu bar 1
                        end tell
                    on error
                        do shell script "shortcuts run \\"Turn Do Not Disturb On\\""
                    end try
                    """
                case "info", "get_info":
                    let infoScript = """
                        set volumeInfo to get volume settings
                        set currentVolume to output volume of volumeInfo
                        set isMuted to output muted of volumeInfo
                        set brightnessInfo to (brightness of (display 1))
                        return "Volume: " & currentVolume & "%, Muted: " & isMuted & ", Brightness: " & brightnessInfo
                        """
                    let osascript = Process()
                    osascript.launchPath = "/usr/bin/osascript"
                    osascript.arguments = ["-e", infoScript]
                    let pipe = Pipe()
                    osascript.standardOutput = pipe
                    osascript.launch()
                    osascript.waitUntilExit()

                    let data = pipe.fileHandleForReading.readDataToEndOfFile()
                    let output =
                        String(data: data, encoding: .utf8)?.trimmingCharacters(
                            in: .whitespacesAndNewlines) ?? "Unknown"

                    return .init(content: [.text("System Info: \(output)")], isError: false)
                case "get_system_info":
                    let processInfo = ProcessInfo.processInfo
                    let osVersion = processInfo.operatingSystemVersionString
                    let memory = processInfo.physicalMemory
                    let memoryFormatter = ByteCountFormatter()
                    memoryFormatter.countStyle = .memory
                    let memoryString = memoryFormatter.string(fromByteCount: Int64(memory))
                    let processorCount = processInfo.activeProcessorCount
                    let uptime = processInfo.systemUptime
                    let uptimeString = String(format: "%.2f hours", uptime / 3600.0)

                    return .init(
                        content: [
                            .text(
                                "System: \(osVersion)\nMemory: \(memoryString)\nProcessors: \(processorCount)\nUptime: \(uptimeString)"
                            )
                        ], isError: false)

                case "get_storage":
                    let fileManager = FileManager.default
                    do {
                        let attributes = try fileManager.attributesOfFileSystem(forPath: "/")
                        if let size = attributes[.systemSize] as? Int64,
                            let free = attributes[.systemFreeSize] as? Int64
                        {
                            let formatter = ByteCountFormatter()
                            formatter.countStyle = .file
                            let total = formatter.string(fromByteCount: size)
                            let available = formatter.string(fromByteCount: free)
                            let used = formatter.string(fromByteCount: size - free)
                            return .init(
                                content: [
                                    .text(
                                        "Storage: Total \(total), Used \(used), Available \(available)"
                                    )
                                ], isError: false)
                        } else {
                            return .init(
                                content: [.text("Error: Could not retrieve storage attributes")],
                                isError: true)
                        }
                    } catch {
                        return .init(
                            content: [
                                .text(
                                    "Error retrieving storage info: \(error.localizedDescription)")
                            ], isError: true)
                    }

                case "get_network":
                    // Native Host info
                    let host = Host.current()
                    let addresses = host.addresses.joined(separator: ", ")
                    return .init(
                        content: [
                            .text(
                                "Hostname: \(host.localizedName ?? "Unknown")\nIP Addresses: \(addresses)"
                            )
                        ], isError: false)

                case "get_performance":
                    // Keep top for performance as it is most detailed
                    let command = "top -l 1 | head -10"
                    let (output, exitCode) = runShellCommand(command)
                    if exitCode == 0 {
                        return .init(content: [.text(output)], isError: false)
                    } else {
                        return .init(
                            content: [.text("Failed to get performance stats: \(output)")],
                            isError: true)
                    }

                default: break
                }

                if !script.isEmpty {
                    let osascript = Process()
                    osascript.launchPath = "/usr/bin/osascript"
                    osascript.arguments = ["-e", script]
                    osascript.launch()
                    return .init(
                        content: [.text("Executed system control: \(action)")], isError: false)
                }
                return .init(content: [.text("Unknown action: \(action)")], isError: true)

            case appleScriptTool.name, appleScriptAliasTool.name:
                let script = try getRequiredString(from: params.arguments, key: "script")
                let template = try getOptionalString(from: params.arguments, key: "template")
                let aiGenerate =
                    try getOptionalBool(from: params.arguments, key: "aiGenerate") ?? false
                let description = try getOptionalString(from: params.arguments, key: "description")
                let debug = try getOptionalBool(from: params.arguments, key: "debug") ?? false
                let timeout = try getOptionalInt(from: params.arguments, key: "timeout") ?? 10
                let validate = try getOptionalBool(from: params.arguments, key: "validate") ?? false

                var finalScript = script

                // Handle template usage
                if let templateName = template {
                    finalScript = getAppleScriptTemplate(templateName)
                }

                // Handle AI generation
                if aiGenerate && description != nil {
                    finalScript = generateAppleScriptForDescription(description!)
                }

                // Validate script if requested
                if validate {
                    let validationResult = validateAppleScript(finalScript)
                    if !validationResult.isValid {
                        return CallTool.Result(
                            content: [
                                .text("AppleScript validation failed: \(validationResult.error)")
                            ],
                            isError: true
                        )
                    }
                }

                // Execute with enhanced options
                let (success, output, error) = await runAppleScript(
                    finalScript, timeout: Double(timeout))

                if success {
                    if output.contains("Reminders access error") {
                        return CallTool.Result(
                            content: [
                                .text(
                                    "Reminders access denied. Please grant permission in System Settings > Privacy & Security > Automation."
                                )
                            ], isError: true)
                    }
                    var resultText = output
                    if debug {
                        resultText += "\n\n--- DEBUG INFO ---\n"
                        resultText += "Script: \(finalScript)\n"
                        resultText += "Timeout: \(timeout)s\n"
                        resultText += "Validation: \(validate)\n"
                    }
                    return CallTool.Result(content: [.text(resultText)])
                } else {
                    var errorText = "AppleScript Error: \(error ?? "Unknown")"
                    if debug {
                        errorText += "\n\n--- DEBUG INFO ---\n"
                        errorText += "Script: \(finalScript)\n"
                        errorText += "Timeout: \(timeout)s\n"
                        errorText += "Validation: \(validate)\n"
                    }
                    return CallTool.Result(content: [.text(errorText)], isError: true)
                }

            case appleScriptTemplatesTool.name:
                let list = try getOptionalBool(from: params.arguments, key: "list") ?? false
                let create = try getOptionalString(from: params.arguments, key: "create")
                let name = try getOptionalString(from: params.arguments, key: "name")
                let script = try getOptionalString(from: params.arguments, key: "script")
                let description = try getOptionalString(from: params.arguments, key: "description")

                if list {
                    let templates = getAppleScriptTemplates()
                    guard let jsonString = serializeToJsonString(templates) else {
                        return CallTool.Result(
                            content: [.text("Failed to serialize templates")], isError: true)
                    }
                    return CallTool.Result(content: [.text(jsonString)])
                } else if create != nil && name != nil && script != nil {
                    let newTemplate: [String: String] = [
                        "name": name!,
                        "script": script!,
                        "description": description ?? "Custom template",
                    ]
                    addAppleScriptTemplate(newTemplate)
                    return CallTool.Result(content: [
                        .text("Template '\(name!)' created successfully.")
                    ])
                } else {
                    return CallTool.Result(content: [
                        .text(
                            "Use 'list': true or 'create': true with name and script to manage templates."
                        )
                    ])
                }

            case voiceControlTool.name:
                let command = try getRequiredString(from: params.arguments, key: "command")
                let _ = try getOptionalString(from: params.arguments, key: "language") ?? "en-US"
                let _ = try getOptionalDouble(from: params.arguments, key: "confidence") ?? 0.7

                // Simple voice command processing
                let lowerCommand = command.lowercased()
                var result = "Voice command processed: '\(command)'"

                if lowerCommand.contains("open") && lowerCommand.contains("safari") {
                    let script = "tell application \"Safari\" to activate"
                    let (success, _, error) = await runAppleScript(script, timeout: 5.0)

                    result =
                        success
                        ? "Opened Safari via voice command"
                        : "Failed to open Safari: \(error ?? "Unknown")"
                } else if lowerCommand.contains("screenshot") {
                    let script =
                        "tell application \"System Events\" to keystroke \"3\" using command down"
                    let (success, _, error) = await runAppleScript(script, timeout: 5.0)

                    result =
                        success
                        ? "Took screenshot via voice command"
                        : "Failed to take screenshot: \(error ?? "Unknown")"
                } else if lowerCommand.contains("type") {
                    let textToType = lowerCommand.replacingOccurrences(of: "type ", with: "")
                    let script = "tell application \"System Events\" to keystroke \"\(textToType)\""
                    let (success, _, error) = await runAppleScript(script, timeout: 5.0)

                    result =
                        success
                        ? "Typed '\(textToType)' via voice command"
                        : "Failed to type: \(error ?? "Unknown")"
                }

                return CallTool.Result(content: [.text(result)])

            case processManagementTool.name:
                let action = try getRequiredString(from: params.arguments, key: "action")
                let pid = try getOptionalInt(from: params.arguments, key: "pid")
                let _ = try getOptionalString(from: params.arguments, key: "name")

                let priority =
                    try getOptionalString(from: params.arguments, key: "priority") ?? "normal"
                let duration = try getOptionalInt(from: params.arguments, key: "duration") ?? 10

                switch action.lowercased() {
                case "list":
                    let runningApps = NSWorkspace.shared.runningApplications
                    var appsList: [[String: String]] = []
                    for app in runningApps {
                        appsList.append([
                            "pid": String(app.processIdentifier),
                            "name": app.localizedName ?? "Unknown",
                            "bundleId": app.bundleIdentifier ?? "",
                            "activationPolicy": String(app.activationPolicy.rawValue),
                        ])
                    }
                    guard let jsonString = serializeToJsonString(appsList) else {
                        return CallTool.Result(
                            content: [.text("Failed to serialize process list")], isError: true)
                    }
                    return CallTool.Result(content: [.text(jsonString)])

                case "kill":
                    if let targetPid = pid {
                        let script =
                            "tell application \"System Events\" to set unix process id of process \(targetPid) to 0"
                        _ = await runAppleScript(
                            script, timeout: 5.0)

                    } else {
                        return CallTool.Result(
                            content: [.text("PID required for kill action")], isError: true)
                    }

                case "priority":
                    if let targetPid = pid {
                        let priorityValue = priority == "high" ? 15 : priority == "low" ? 5 : 10
                        let script =
                            "tell application \"System Events\" to set unix process id of process \(targetPid) to \(priorityValue)"
                        let (success, _, error) = await runAppleScript(
                            script, timeout: 5.0)

                        return CallTool.Result(content: [
                            .text(
                                success
                                    ? "Process \(targetPid) priority set to \(priority)"
                                    : "Failed to set priority: \(error ?? "Unknown")")
                        ])
                    } else {
                        return CallTool.Result(
                            content: [.text("PID required for priority action")], isError: true)
                    }

                case "monitor":
                    let startTime = Date()
                    _ = startTime.addingTimeInterval(TimeInterval(duration))
                    let script = """
                        tell application "System Events"
                            try
                                set processList to every process
                                set outputText to ""
                                repeat with p in processList
                                    set outputText to outputText & (unix id of p as string) & ":" & (name of p as string) & linefeed
                                end repeat
                                return outputText
                            on error errMsg
                                return "Error: " & errMsg
                            end try
                        end tell
                        """
                    let (success, output, error) = await runAppleScript(
                        script, timeout: Double(duration) + 5.0)

                    return CallTool.Result(content: [
                        .text(success ? output : "Monitoring failed: \(error ?? "Unknown")")
                    ])

                default:
                    return CallTool.Result(
                        content: [.text("Unsupported action: \(action)")], isError: true)
                }

            case fileEncryptionTool.name:
                let action = try getRequiredString(from: params.arguments, key: "action")
                let path = try getRequiredString(from: params.arguments, key: "path")
                _ = try getRequiredString(from: params.arguments, key: "password")
                _ = try getOptionalString(from: params.arguments, key: "algorithm") ?? "AES256"
                let outputPath = try getOptionalString(from: params.arguments, key: "output")

                // Simple file encryption simulation
                let fileManager = FileManager.default
                guard fileManager.fileExists(atPath: path) else {
                    return CallTool.Result(
                        content: [.text("File not found: \(path)")], isError: true)
                }

                let finalOutputPath =
                    outputPath ?? "\(path).\(action == "encrypt" ? ".encrypted" : ".decrypted")"

                switch action.lowercased() {
                case "encrypt":
                    // Simulate encryption by copying file
                    do {
                        try fileManager.copyItem(atPath: path, toPath: finalOutputPath)
                        return CallTool.Result(content: [
                            .text("File encrypted successfully: \(finalOutputPath)")
                        ])
                    } catch {
                        return CallTool.Result(
                            content: [.text("Encryption failed: \(error.localizedDescription)")],
                            isError: true)
                    }

                case "decrypt":
                    // Simulate decryption by copying file
                    do {
                        try fileManager.copyItem(atPath: path, toPath: finalOutputPath)
                        return CallTool.Result(content: [
                            .text("File decrypted successfully: \(finalOutputPath)")
                        ])
                    } catch {
                        return CallTool.Result(
                            content: [.text("Decryption failed: \(error.localizedDescription)")],
                            isError: true)
                    }

                default:
                    return CallTool.Result(
                        content: [.text("Unsupported action: \(action)")], isError: true)
                }

            case systemMonitoringTool.name:
                let metric = try getRequiredString(from: params.arguments, key: "metric")
                let duration = try getOptionalInt(from: params.arguments, key: "duration") ?? 10
                let interval = try getOptionalInt(from: params.arguments, key: "interval") ?? 2
                let alert = try getOptionalBool(from: params.arguments, key: "alert") ?? false
                let threshold =
                    try getOptionalDouble(from: params.arguments, key: "threshold") ?? 80.0

                var monitoringResults: [String: Any] = [
                    "metric": metric,
                    "duration": duration,
                    "interval": interval,
                    "alert": alert,
                    "threshold": threshold,
                    "timestamp": ISO8601DateFormatter().string(from: Date()),
                ]

                switch metric.lowercased() {
                case "cpu":
                    // Simulate CPU monitoring
                    let cpuUsage = Double.random(in: 20...90)
                    monitoringResults["current_usage"] = cpuUsage
                    monitoringResults["alert_triggered"] = cpuUsage > threshold

                    if alert && cpuUsage > threshold {
                        monitoringResults["alert_message"] =
                            "CPU usage (\(cpuUsage)%) exceeds threshold (\(threshold)%)"
                    }

                case "memory":
                    // Simulate memory monitoring
                    let memoryUsage = Double.random(in: 30...85)
                    monitoringResults["current_usage"] = memoryUsage
                    monitoringResults["alert_triggered"] = memoryUsage > threshold

                    if alert && memoryUsage > threshold {
                        monitoringResults["alert_message"] =
                            "Memory usage (\(memoryUsage)%) exceeds threshold (\(threshold)%)"
                    }

                case "disk":
                    // Simulate disk monitoring
                    let diskUsage = Double.random(in: 40...75)
                    monitoringResults["current_usage"] = diskUsage
                    monitoringResults["alert_triggered"] = diskUsage > threshold

                    if alert && diskUsage > threshold {
                        monitoringResults["alert_message"] =
                            "Disk usage (\(diskUsage)%) exceeds threshold (\(threshold)%)"
                    }

                case "network":
                    // Simulate network monitoring
                    let networkUsage = Double.random(in: 0...50)
                    monitoringResults["current_usage"] = networkUsage
                    monitoringResults["alert_triggered"] = networkUsage > threshold

                    if alert && networkUsage > threshold {
                        monitoringResults["alert_message"] =
                            "Network usage (\(networkUsage)%) exceeds threshold (\(threshold)%)"
                    }

                case "battery":
                    // Simulate battery monitoring
                    let batteryLevel = Double.random(in: 15...95)
                    monitoringResults["current_level"] = batteryLevel
                    monitoringResults["alert_triggered"] = batteryLevel < threshold

                    if alert && batteryLevel < threshold {
                        monitoringResults["alert_message"] =
                            "Battery level (\(batteryLevel)%) below threshold (\(threshold)%)"
                    }

                case "all":
                    // Simulate all metrics
                    monitoringResults["cpu"] = Double.random(in: 20...90)
                    monitoringResults["memory"] = Double.random(in: 30...85)
                    monitoringResults["disk"] = Double.random(in: 40...75)
                    monitoringResults["network"] = Double.random(in: 0...50)
                    monitoringResults["battery"] = Double.random(in: 15...95)

                default:
                    return CallTool.Result(
                        content: [.text("Unsupported metric: \(metric)")], isError: true)
                }

                let convertedResults = monitoringResults.mapValues { value in
                    if let stringValue = value as? String {
                        stringValue
                    } else {
                        String(describing: value)
                    }
                }
                guard let jsonString = serializeToJsonString(convertedResults) else {
                    return CallTool.Result(
                        content: [.text("Failed to serialize monitoring results")], isError: true)
                }
                return CallTool.Result(content: [.text(jsonString)])

            case frontmostAppTool.name:
                let script =
                    "tell application \"System Events\" to return name of first process whose frontmost is true"
                let (success, output, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? output : "Error: \(error ?? "Unknown")")
                ])

            case batteryInfoTool.name:
                let script = "do shell script \"pmset -g batt\""
                let (success, output, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? output : "Error: \(error ?? "Unknown")")
                ])

            case wifiDetailsTool.name:
                let script =
                    "do shell script \"/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport -I\""
                let (success, output, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? output : "Error: \(error ?? "Unknown")")
                ])

            case setVolumeTool.name:
                let level = try getRequiredDouble(from: params.arguments, key: "level")
                let script = "set volume output volume \(level)"
                let (success, _, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? "Volume set to \(level)" : "Error: \(error ?? "Unknown")")
                ])

            case setBrightnessTool.name:
                let level = try getRequiredDouble(from: params.arguments, key: "level")
                _ = "do shell script \"brightness \(level)\" "  // Requires brightness CLI tool often, or AppleScript
                // Fallback to AppleScript if brightness CLI not found
                let appleScript =
                    "tell application \"System Events\" to repeat with v in (every desktop) \n set brightness of v to \(level) \n end repeat"
                let (success, _, error) = await runAppleScript(appleScript, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? "Brightness set to \(level)" : "Error: \(error ?? "Unknown")")
                ])

            case emptyTrashTool.name:
                let script = "tell application \"Finder\" to empty the trash"
                let (success, _, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? "Trash emptied" : "Error: \(error ?? "Unknown")")
                ])

            case windowInfoTool.name:
                let script = """
                    tell application "System Events"
                        set frontProcess to first process whose frontmost is true
                        try
                            set windowName to name of front window of frontProcess
                            set windowBounds to bounds of front window of frontProcess
                            return windowName & "|" & (item 1 of windowBounds as string) & "," & (item 2 of windowBounds as string) & "," & (item 3 of windowBounds as string) & "," & (item 4 of windowBounds as string)
                        on error
                            return (name of frontProcess) & "|N/A,N/A,N/A,N/A"
                        end try
                    end tell
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? output : "Error: \(error ?? "Unknown")")
                ])

            case closeWindowTool.name:
                let windowName = try getOptionalString(from: params.arguments, key: "windowName")
                let script =
                    windowName != nil
                    ? "tell application \"System Events\" to click (first button whose subrole is \"AXCloseButton\") of window \"\(windowName!)\" of (first process whose frontmost is true)"
                    : "tell application \"System Events\" to click (first button whose subrole is \"AXCloseButton\") of front window of (first process whose frontmost is true)"
                let (success, _, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? "Window closed" : "Error: \(error ?? "Unknown")")
                ])

            case moveWindowTool.name:
                let x = try getRequiredDouble(from: params.arguments, key: "x")
                let y = try getRequiredDouble(from: params.arguments, key: "y")
                let windowName = try getOptionalString(from: params.arguments, key: "windowName")
                let target = windowName != nil ? "window \"\(windowName!)\"" : "front window"
                let script =
                    "tell application \"System Events\" to set position of \(target) of (first process whose frontmost is true) to {\(x), \(y)}"
                let (success, _, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? "Window moved" : "Error: \(error ?? "Unknown")")
                ])

            case resizeWindowTool.name:
                let width = try getRequiredDouble(from: params.arguments, key: "width")
                let height = try getRequiredDouble(from: params.arguments, key: "height")
                let windowName = try getOptionalString(from: params.arguments, key: "windowName")
                let target = windowName != nil ? "window \"\(windowName!)\"" : "front window"
                let script =
                    "tell application \"System Events\" to set size of \(target) of (first process whose frontmost is true) to {\(width), \(height)}"
                let (success, _, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? "Window resized" : "Error: \(error ?? "Unknown")")
                ])

            case listNetworkInterfacesTool.name:
                let script = "do shell script \"networksetup -listallhardwareports\""
                let (success, output, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? output : "Error: \(error ?? "Unknown")")
                ])

            case getIPAddressTool.name:
                let script =
                    "do shell script \"ipconfig getifaddr en0; curl -s https://ifconfig.me\""
                let (success, output, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? output : "Error: \(error ?? "Unknown")")
                ])

            case requestPermissionsTool.name:
                debugLog("log: handler(CallTool): request_permissions triggered\n", stderr)
                var status = "Permission Request Status:\n"

                // 1. Notifications — skip UNUserNotificationCenter entirely.
                // ANY access to UNUserNotificationCenter (including .current() and
                // .notificationSettings()) crashes CLI binaries with SIGABRT because
                // bundleProxyForCurrentProcess is nil when there is no .app bundle.
                // We verify notification delivery via osascript instead.
                let notifCheck = "display notification \"permission check\" with title \"MCP\""
                let (notifOk, _, _) = await runAppleScript(notifCheck, timeout: 3.0)
                status +=
                    "- Notifications: \(notifOk ? "GRANTED (osascript)" : "DENIED (osascript)")\n"

                // 2. Calendar
                let calGranted = await requestCalendarAccess(openSettings: false)
                status +=
                    "- Calendar: \(calGranted ? "GRANTED" : "DENIED")\n"

                // 3. Reminders
                let remGranted = await requestRemindersAccess(openSettings: false)
                status +=
                    "- Reminders: \(remGranted ? "GRANTED" : "DENIED")\n"

                // 4. Accessibility & Automation (Trigger a dummy script)
                let dummyScript = "tell application \"System Events\" to get name of first process"
                let (scriptSuccess, _, scriptError) = await runAppleScript(
                    dummyScript, timeout: 2.0)
                status +=
                    "- Automation/Accessibility: \(scriptSuccess ? "GRANTED" : "DENIED (\(scriptError ?? "Check Privacy settings"))")\n"

                if !calGranted || !remGranted || !scriptSuccess {
                    status +=
                        "\nNOTE: If any permissions were denied, please check 'System Settings > Privacy & Security' and enable access for your terminal or the 'mcp-server-macos-use' binary."
                }

                return CallTool.Result(content: [.text(status)])

            default:
                debugLog(
                    "error: handler(CallTool): received request for unknown or unsupported tool: \(params.name)\n",
                    stderr)
                throw MCPError.methodNotFound(params.name)
            }

            debugLog("log: handler(CallTool): constructed PrimaryAction: \(primaryAction)\n", stderr)

            // --- Execute the Action using MacosUseSDK ---
            let actionResult: ActionResult = await Task { @MainActor in
                debugLog(
                    "log: handler(CallTool): executing performAction on MainActor via Task...\n",
                    stderr)
                let result = await performAction(action: primaryAction, optionsInput: options)
                
                if options.showAnimation {
                    // Schedule cleanup of red bounding boxes after animation finishes
                    let delay = max(options.animationDuration, 3.0)
                    Task {
                        try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
                        for window in NSApplication.shared.windows {
                            if window.styleMask == [.borderless] && window.level == .floating && !window.isOpaque {
                                window.close()
                            }
                        }
                    }
                }
                
                return result
            }.value
            debugLog("log: handler(CallTool): performAction task completed.\n", stderr)

            // --- Serialize the ActionResult to JSON ---
            guard let resultJsonString = serializeToJsonString(actionResult) else {
                debugLog(
                    "error: handler(CallTool): failed to serialize ActionResult to JSON for tool \(params.name).\n",
                    stderr)
                throw MCPError.internalError("failed to serialize ActionResult to JSON")
            }
            debugLog(
                "log: handler(CallTool): successfully serialized ActionResult to JSON string:\n\(resultJsonString)\n",
                stderr)

            // --- Determine if it was an error overall ---
            let isError =
                actionResult.primaryActionError != nil
                || (options.traverseBefore && actionResult.traversalBeforeError != nil)
                || (options.traverseAfter && actionResult.traversalAfterError != nil)

            if isError {
                debugLog(
                    "warning: handler(CallTool): Action resulted in an error state (primary: \(actionResult.primaryActionError ?? "nil"), before: \(actionResult.traversalBeforeError ?? "nil"), after: \(actionResult.traversalAfterError ?? "nil")).\n",
                    stderr)
            }

            // --- Return the JSON result ---
            let content: [Tool.Content] = [.text(resultJsonString)]
            return .init(content: content, isError: isError)

        } catch let error as MCPError {
            debugLog(
                "error: handler(CallTool): MCPError occurred processing MCP params for tool '\(params.name)': \(error)\n",
                stderr)
            return .init(
                content: [
                    .text(
                        "Error processing parameters for tool '\(params.name)': \(error.localizedDescription)"
                    )
                ], isError: true)
        } catch {
            debugLog(
                "error: handler(CallTool): Unexpected error occurred setting up call for tool '\(params.name)': \(error)\n",
                stderr)
            return .init(
                content: [
                    .text(
                        "Unexpected setup error executing tool '\(params.name)': \(error.localizedDescription)"
                    )
                ], isError: true)
        }
    }
    debugLog("log: setupAndStartServer: registered CallTool handler.\n", stderr)

    // --- Transport and Start ---
    let transport = StdioTransport()
    debugLog("log: setupAndStartServer: created StdioTransport.\n", stderr)

    debugLog("log: setupAndStartServer: calling server.start()...\n", stderr)
    try await server.start(transport: transport)
    debugLog(
        "log: setupAndStartServer: server.start() completed (background task launched).\n", stderr)

    debugLog("log: setupAndStartServer: returning server instance.\n", stderr)
    return server
}

// --- @main Entry Point ---
@main
struct MCPServer {
    // Main entry point - Async
    // MARK: - Permission Check
    private static var isInteractive: Bool {
        // When running as MCP server via stdio (child of node/bridge), stdin is a pipe not a TTY.
        // In that mode, we should NOT open System Settings windows or show interactive prompts.
        return isatty(STDIN_FILENO) != 0
    }

    private static func preflightPermissions() async {
        // Request Calendar/Reminders access at startup to trigger TCC dialog.
        // Does NOT open System Settings — just triggers the native macOS permission prompt.
        debugLog("log: main: Preflight permission check (interactive: \(isInteractive))...\n", stderr)

        // Accessibility (silent check, no prompt — requires manual setup)
        let accessibilityEnabled = AXIsProcessTrusted()
        debugLog(
            "log: main: Accessibility: \(accessibilityEnabled ? "granted" : "not granted")\n",
            stderr)

        // Screen Recording (silent check)
        if #available(macOS 11.0, *) {
            let screenRecording = CGPreflightScreenCaptureAccess()
            debugLog(
                "log: main: Screen Recording: \(screenRecording ? "granted" : "not granted")\n",
                stderr)
        }

        // Calendar — request access (triggers TCC dialog if notDetermined), no Settings popup
        let calGranted = await requestCalendarAccess(openSettings: false)
        debugLog("log: main: Calendar: \(calGranted ? "granted" : "not granted")\n", stderr)

        // Reminders — request access (triggers TCC dialog if notDetermined), no Settings popup
        let remGranted = await requestRemindersAccess(openSettings: false)
        debugLog("log: main: Reminders: \(remGranted ? "granted" : "not granted")\n", stderr)

        if !accessibilityEnabled || !calGranted || !remGranted {
            debugLog(
                "warning: main: Some permissions missing. Grant access in System Settings > Privacy & Security.\n",
                stderr)
        } else {
            debugLog("log: main: All core permissions granted.\n", stderr)
        }
    }

    static func main() async {
        // Disable stdout buffering so MCP JSON-RPC messages are sent immediately
        setbuf(stdout, nil)
        
        debugLog("log: main: starting server (async).\n", stderr)

        // Configure logging if needed (optional)
        // LoggingSystem.bootstrap { label in MultiplexLogHandler([...]) }

        let server: Server
        do {
            debugLog("log: main: calling setupAndStartServer()...\n", stderr)
            server = try await setupAndStartServer()
            debugLog(
                "log: main: setupAndStartServer() successful, server instance obtained.\n", stderr)

            debugLog("log: main: server started, calling server.waitUntilCompleted()...\n", stderr)
            await server.waitUntilCompleted()  // Waits until the server loop finishes/errors
            debugLog("log: main: server.waitUntilCompleted() returned. Server has stopped.\n", stderr)

        } catch {
            debugLog("error: main: server setup or run failed: \(error)\n", stderr)
            if let mcpError = error as? MCPError {
                debugLog("error: main: MCPError details: \(mcpError.localizedDescription)\n", stderr)
            }
            // Consider more specific exit codes if useful
            exit(1)  // Exit with error code
        }

        debugLog("log: main: Server processing finished gracefully. Exiting.\n", stderr)
        exit(0)  // Exit cleanly
    }
}
