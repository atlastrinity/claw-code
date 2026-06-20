import AppKit

struct MCPServer {
    static func main() async {
        print("Start")
        await Task { @MainActor in
            print("Getting shared app")
            let app = NSApplication.shared
            print("Getting windows")
            let windows = app.windows
            print("Got \(windows.count) windows")
        }.value
        print("End")
    }
}
await MCPServer.main()
