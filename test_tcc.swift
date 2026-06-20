import CoreGraphics
if #available(macOS 11.0, *) {
    print("Preflight: \(CGPreflightScreenCaptureAccess())")
}
