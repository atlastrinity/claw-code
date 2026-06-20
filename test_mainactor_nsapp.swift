import AppKit

func test() async {
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

Task {
    await test()
    exit(0)
}
dispatchMain()
