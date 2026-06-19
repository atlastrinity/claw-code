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
import ScreenCaptureKit

// --- Persistent State ---
nonisolated(unsafe) var persistentCWD: String = FileManager.default.currentDirectoryPath

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
nonisolated(unsafe) var visionCache: [String: VisionCacheEntry] = [:]

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



// --- Interactive Environment Check ---
let isInteractive: Bool = {
    // When running as MCP server via stdio (child of node/bridge), stdin is a pipe not a TTY.
    // In that mode, we should NOT open System Settings windows or show interactive prompts.
    return isatty(STDIN_FILENO) != 0
}()

// --- Persistent EventStore ---
nonisolated(unsafe) let eventStore = EKEventStore()

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

func captureMainDisplay(monitor: Int? = nil) async -> CGImage? {
    guard #available(macOS 14.1, *) else {
        guard let monitorIndex = monitor, monitorIndex > 0 else {
            return CGDisplayCreateImage(CGMainDisplayID())
        }
        var displayCount: UInt32 = 0
        CGGetActiveDisplayList(0, nil, &displayCount)
        guard displayCount > 0 else { return CGDisplayCreateImage(CGMainDisplayID()) }
        var displays = [CGDirectDisplayID](repeating: 0, count: Int(displayCount))
        CGGetActiveDisplayList(displayCount, &displays, &displayCount)
        let idx = min(monitorIndex, Int(displayCount) - 1)
        return CGDisplayCreateImage(displays[idx])
    }
    
    do {
        let content = try await SCShareableContent.current
        var targetDisplay = content.displays.first
        if let monitorIndex = monitor, monitorIndex > 0, monitorIndex < content.displays.count {
            targetDisplay = content.displays[monitorIndex]
        }
        guard let display = targetDisplay else { return nil }
        
        let config = SCStreamConfiguration()
        config.width = display.width
        config.height = display.height
        let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
        return try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
    } catch {
        debugLog("error: SCK captureMainDisplay: \(error)\n", stderr)
        return nil
    }
}

/// Captures only the window owned by the given PID, instead of the full screen.
/// Returns nil if no on-screen window is found for this PID.
/// This produces a much smaller image than full-screen capture, saving tokens.
func captureWindow(pid: pid_t) async -> CGImage? {
    guard #available(macOS 14.1, *) else {
        guard let windowList = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
            return nil
        }
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
    
    do {
        let content = try await SCShareableContent.current
        guard content.applications.contains(where: { $0.processID == pid }) else { return nil }
        
        let appWindows = content.windows.filter { $0.owningApplication?.processID == pid && $0.isOnScreen }
        
        var bestWindow: SCWindow? = nil
        var bestArea: CGFloat = 0
        for window in appWindows {
            let area = window.frame.width * window.frame.height
            if area > bestArea {
                bestArea = area
                bestWindow = window
            }
        }
        
        guard let targetWindow = bestWindow else { return nil }
        
        let config = SCStreamConfiguration()
        config.width = Int(targetWindow.frame.width * 2) // Approximate backing scale
        config.height = Int(targetWindow.frame.height * 2)
        
        let filter = SCContentFilter(desktopIndependentWindow: targetWindow)
        return try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
    } catch {
        debugLog("error: SCK captureWindow: \(error)\n", stderr)
        return nil
    }
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
                "description": .string("REQUIRED. Absolute pixel X coordinate for the click (e.g., 960). DO NOT use normalized 0.0-1.0 values."),
            ]),
            "y": .object([
                "type": .string("number"),
                "description": .string("REQUIRED. Absolute pixel Y coordinate for the click (e.g., 1000). DO NOT use normalized 0.0-1.0 values."),
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
                "description": .string("PID of target app. Pass a valid PID to capture only that app's window (saves tokens). Pass 0 to capture the current frontmost app. If omitted, captures the ENTIRE SCREEN (expensive)."),
            ]),
            "diff": .object([
                "type": .string("boolean"),
                "description": .string("When true, checks if the screen has visually changed since the last capture using a perceptual hash. If no change, returns a minimal 'no_change' response instead of the full image/OCR, saving 99% tokens (default: true)."),
            ]),
        ]),
    ])
    let unifiedVisionTool = {
        let screenCount = NSScreen.screens.count
        let monitorIndices = (0..<screenCount).map { String($0) }.joined(separator: ", ")
        
        let dynamicDescription = """
        Unified vision tool. 
        CRITICAL: There are currently \(screenCount) monitor(s) connected (available indices: \(monitorIndices)). 
        Always prefer capturing a specific application window (using the pid parameter, e.g., pid: 0 for frontmost app) instead of the full screen to save API costs. 
        Only use full screen (monitor parameter) if absolutely necessary.
        Modes: 'smart' (auto-decides if image needed based on OCR richness), 'ocr', 'image', 'full'. Supports region, language hints, and accessibility tree.
        NOTE: 'diff' is true by default to save tokens. If you MUST force a fresh image capture, explicitly pass diff: false.
        """
        return Tool(
            name: "macos-use_vision",
            description: dynamicDescription,
            inputSchema: unifiedVisionSchema
        )
    }()

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
                "type": .string("number"), "description": .string("REQUIRED. Absolute pixel Screen X coordinate. DO NOT use normalized 0.0-1.0 values."),
            ]),
            "y": .object([
                "type": .string("number"), "description": .string("REQUIRED. Absolute pixel Screen Y coordinate. DO NOT use normalized 0.0-1.0 values."),
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

    // *** NEW: Triple Click Tool (select entire line) ***

    // *** NEW: Mouse Move Tool ***

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
                "type": .string("number"), "description": .string("REQUIRED. Absolute pixel Start X coordinate. DO NOT use normalized 0.0-1.0 values."),
            ]),
            "startY": .object([
                "type": .string("number"), "description": .string("REQUIRED. Absolute pixel Start Y coordinate. DO NOT use normalized 0.0-1.0 values."),
            ]),
            "endX": .object([
                "type": .string("number"), "description": .string("REQUIRED. Absolute pixel End X coordinate. DO NOT use normalized 0.0-1.0 values."),
            ]),
            "endY": .object([
                "type": .string("number"), "description": .string("REQUIRED. Absolute pixel End Y coordinate. DO NOT use normalized 0.0-1.0 values."),
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

    let windowMgmtSchema: Value = .object([
        "type": .string("object"),
        "properties": .object([
            "pid": .object([
                "type": .string("number"),
                "description": .string(
                    "OPTIONAL. PID of the application. Defaults to frontmost app."),
            ]),
            "windowIndex": .object([
                "type": .string("number"),
                "description": .string(
                    "Optional. Index of the window (0-based) to target. If omitted, targets the focused window."
                ),
            ]),
            "action": .object([
                "type": .string("string"),
                "description": .string(
                    "Action: 'move', 'resize', 'minimize', 'maximize', 'make_front', 'snapshot', 'group', 'ungroup', 'close'."
                ),
                "enum": .array([
                    .string("move"), .string("resize"), .string("minimize"),
                    .string("maximize"), .string("make_front"), .string("snapshot"),
                    .string("group"), .string("ungroup"), .string("close"),
                ]),
            ]),
            "x": .object([
                "type": .string("number"), "description": .string("Optional absolute pixel X for move. DO NOT use normalized values."),
            ]),
            "y": .object([
                "type": .string("number"), "description": .string("Optional absolute pixel Y for move. DO NOT use normalized values."),
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



    // *** NEW: Voice Control Tool ***

    // *** NEW: Process Management Tool ***

    // *** NEW: File Encryption Tool ***

    // *** NEW: System Monitoring Tool ***

    // *** ENHANCED: System Control Tool ***

    // *** NEW: Fetch URL Tool ***

    // *** ENHANCED: Time Tools ***


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


    // *** NEW: Calendar Tools ***

    // *** ENHANCED: Calendar Event Creation ***

    // *** NEW: Reminder Tools ***


    // *** NEW: Spotlight Tool ***

    // *** ENHANCED: Notification Tool ***


    // *** NEW: Apple Notes Tools ***



    // *** ENHANCED: Apple Mail Tools ***


    // *** ENHANCED: Finder Tools ***




    // *** NEW: List Running Applications ***

    // *** NEW: List Browser Tabs ***

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

    // *** NEW: Frontmost App Tool ***

    // *** NEW: Battery Info Tool ***

    // *** NEW: WiFi Details Tool ***

    // *** NEW: Set System Volume Tool ***

    // *** NEW: Set Screen Brightness Tool ***

    // *** NEW: Empty Trash Tool ***

    // *** NEW: Window Info Tool ***
    let windowInfoTool = Tool(
        name: "macos-use_get_active_window_info",
        description: "Returns detailed information about the frontmost window.",
        inputSchema: .object(["type": .string("object"), "properties": .object([:])])
    )

    // *** NEW: Close Window Tool ***

    // *** NEW: Move Window Tool ***

    // *** NEW: Resize Window Tool ***

    // *** NEW: List Network Interfaces Tool ***

    // *** NEW: Get IP Address Tool ***

    // *** NEW: Request Permissions Tool ***

    // *** ALIAS TOOLS FOR naming compatibility ***

    // --- Aggregate list of tools ---
    let allTools = [
        unifiedVisionTool,
        refreshTool,
        clickTool,
        rightClickTool,
        typeTool,
        pressKeyTool,
        scrollTool,
        dragDropTool,
        windowMgmtTool,
        listWindowsTool,
        appleScriptTool,
        windowInfoTool
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
            
            let startX = try getOptionalDouble(from: params.arguments, key: "startX")
            let argX = try getOptionalDouble(from: params.arguments, key: "x")
            let coordX = startX ?? argX
            
            let startY = try getOptionalDouble(from: params.arguments, key: "startY")
            let argY = try getOptionalDouble(from: params.arguments, key: "y")
            let coordY = startY ?? argY
            
            let xArg = coordX != nil ? CGFloat(coordX!) : nil
            let yArg = coordY != nil ? CGFloat(coordY!) : nil
            let resolvedPid = resolvePid(pidOptionalInt, x: xArg, y: yArg)

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
                    rightClickTool.name, dragDropTool.name
                ]
                if toolsWithActivation.contains(params.name) {
                    if let app = NSRunningApplication(processIdentifier: pid_t(convertedPid)) {
                        app.activate(options: .activateIgnoringOtherApps)
                        let delayNanoseconds: UInt64 = (params.name == dragDropTool.name) ? 1_000_000_000 : 250_000_000
                        try? await Task.sleep(nanoseconds: delayNanoseconds)
                    }
                }
            }

            switch params.name {
            case clickTool.name:
                let x = try getRequiredDouble(from: params.arguments, key: "x")
                let y = try getRequiredDouble(from: params.arguments, key: "y")
                primaryAction = .input(action: .click(point: CGPoint(x: x, y: y)))
                options.pidForTraversal = convertedPid  // Re-affirm

            case typeTool.name:
                let text = try getRequiredString(from: params.arguments, key: "text")
                
                // Use CGEvent to send Unicode strings directly. This types letter-by-letter
                // and perfectly handles any keyboard layout (e.g. Ukrainian/English).
                if let source = CGEventSource(stateID: .hidSystemState) {
                    for char in text.utf16 {
                        var uniChar = char
                        if let eventDown = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true) {
                            eventDown.keyboardSetUnicodeString(stringLength: 1, unicodeString: &uniChar)
                            eventDown.post(tap: .cghidEventTap)
                        }
                        if let eventUp = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false) {
                            eventUp.keyboardSetUnicodeString(stringLength: 1, unicodeString: &uniChar)
                            eventUp.post(tap: .cghidEventTap)
                        }
                        usleep(5000) // 5ms delay between keystrokes
                    }
                }
                
                primaryAction = .traverseOnly
                options.pidForTraversal = convertedPid  // Re-affirm

            // ... (Other existing cases) ...

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
                        window["pid"] = String(
                            describing: windowInfo[kCGWindowOwnerPID as String] ?? 0)
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
                if let pidArg = targetPid {
                    let actualPid = resolvePid(pidArg)
                    if actualPid > 0, let convertedPid = pid_t(exactly: actualPid) {
                        capturedImage = await captureWindow(pid: convertedPid)
                        usedWindowCapture = (capturedImage != nil)
                    }
                }
                // Fallback to full-screen capture
                if capturedImage == nil {
                    capturedImage = await captureMainDisplay(monitor: monitor)
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
                let diff = try getOptionalBool(from: params.arguments, key: "diff") ?? true
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

            case dragDropTool.name:
                let startXRaw = try getRequiredDouble(from: params.arguments, key: "startX")
                let startYRaw = try getRequiredDouble(from: params.arguments, key: "startY")
                let endX = try getRequiredDouble(from: params.arguments, key: "endX")
                let endY = try getRequiredDouble(from: params.arguments, key: "endY")
                let steps = try getOptionalInt(from: params.arguments, key: "steps") ?? 10

                // Adjust start coordinates if they hit the traffic lights
                let (startX, startY) = adjustDragStartCoordinateIfNeeded(x: startXRaw, y: startYRaw)

                // Apply the same offset to end coordinates so the total drag vector remains what the AI intended
                let deltaX = startX - startXRaw
                let deltaY = startY - startYRaw
                let finalEndX = endX + deltaX
                let finalEndY = endY + deltaY

                let start = CGPoint(x: startX, y: startY)
                let end = CGPoint(x: finalEndX, y: finalEndY)

                // Move mouse to start position and wait ~1 second before grabbing
                let mouseMove = CGEvent(
                    mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: start,
                    mouseButton: .left)
                mouseMove?.post(tap: .cghidEventTap)
                try? await Task.sleep(nanoseconds: 1_000_000_000)  // 1s pause

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
                    let currentX = startX + (finalEndX - startX) * t
                    let currentY = startY + (finalEndY - startY) * t
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

                let windowIndex = try? getOptionalInt(from: params.arguments, key: "windowIndex")

                let appRef = AXUIElementCreateApplication(pid_t(convertedPid))
                var windowValue: AnyObject?
                var result: AXError = .failure

                if let index = windowIndex {
                    var windowsValue: AnyObject?
                    result = AXUIElementCopyAttributeValue(appRef, kAXWindowsAttribute as CFString, &windowsValue)
                    if result == .success, let windows = windowsValue as? [AXUIElement], index >= 0 && index < windows.count {
                        windowValue = windows[index]
                    } else {
                        result = .failure
                    }
                } else {
                    result = AXUIElementCopyAttributeValue(
                        appRef, kAXFocusedWindowAttribute as CFString, &windowValue)
                }

                if result == .success, let window = windowValue as! AXUIElement? {
                    switch action {
                    case "close":
                        var closeButton: AnyObject?
                        let closeResult = AXUIElementCopyAttributeValue(
                            window, kAXCloseButtonAttribute as CFString, &closeButton)
                        if closeResult == .success, let button = closeButton as! AXUIElement? {
                            AXUIElementPerformAction(button, kAXPressAction as CFString)
                        } else {
                            let app = NSRunningApplication(processIdentifier: pid_t(convertedPid))
                            if let appName = app?.localizedName {
                                let script = "tell application \"System Events\" to click (first button whose subrole is \"AXCloseButton\") of front window of process \"\(appName)\""
                                _ = await runAppleScript(script, timeout: 5.0)
                            }
                        }
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

            case appleScriptTool.name:
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

            case windowInfoTool.name:
                let script = """
                    tell application "System Events"
                        set frontProcess to first process whose frontmost is true
                        try
                            set windowName to name of front window of frontProcess
                            try
                                set windowBounds to bounds of front window of frontProcess
                                return windowName & "|" & (item 1 of windowBounds as string) & "," & (item 2 of windowBounds as string) & "," & (item 3 of windowBounds as string) & "," & (item 4 of windowBounds as string)
                            on error
                                set wPos to position of front window of frontProcess
                                set wSize to size of front window of frontProcess
                                return windowName & "|" & (item 1 of wPos as string) & "," & (item 2 of wPos as string) & "," & (((item 1 of wPos) + (item 1 of wSize)) as string) & "," & (((item 2 of wPos) + (item 2 of wSize)) as string)
                            end try
                        on error
                            return (name of frontProcess) & "|N/A,N/A,N/A,N/A"
                        end try
                    end tell
                    """
                let (success, output, error) = await runAppleScript(script, timeout: 5.0)
                return CallTool.Result(content: [
                    .text(success ? output : "Error: \(error ?? "Unknown")")
                ])

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

// --- Entry Point ---
struct MCPServer {
    // Main entry point - Async
    // MARK: - Permission Check
    private static let isInteractive: Bool = {
        // When running as MCP server via stdio (child of node/bridge), stdin is a pipe not a TTY.
        // In that mode, we should NOT open System Settings windows or show interactive prompts.
        return isatty(STDIN_FILENO) != 0
    }()

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

// Run the server
await MCPServer.main()
