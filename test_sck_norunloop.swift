import ScreenCaptureKit
import CoreGraphics

struct MCPServer {
    static func main() async {
        print("Testing SCK without dispatchMain...")
        do {
            let content = try await SCShareableContent.current
            print("Got content, displays: \(content.displays.count)")
            if let display = content.displays.first {
                let config = SCStreamConfiguration()
                config.width = display.width
                config.height = display.height
                let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
                let img = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
                print("Capture success")
            }
        } catch {
            print("Error: \(error)")
        }
    }
}
await MCPServer.main()
