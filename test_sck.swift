import Foundation
import ScreenCaptureKit
import CoreGraphics

if #available(macOS 14.1, *) {
    Task {
        do {
            let content = try await SCShareableContent.current
            guard let display = content.displays.first else {
                print("No displays")
                exit(1)
            }
            let config = SCStreamConfiguration()
            config.width = display.width
            config.height = display.height
            let image = try await SCScreenshotManager.captureImage(contentFilter: SCContentFilter(display: display, excludingApplications: [], exceptingWindows: []), configuration: config)
            print("Captured SCK image: \(image.width)x\(image.height)")
            exit(0)
        } catch {
            print("Error: \(error)")
            exit(1)
        }
    }
    RunLoop.main.run(until: Date(timeIntervalSinceNow: 5))
} else {
    print("macOS 14.1+ required")
}
