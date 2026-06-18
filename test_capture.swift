import CoreGraphics
import Foundation

if let image = CGDisplayCreateImage(CGMainDisplayID()) {
    print("Success: \(image.width)x\(image.height)")
} else {
    print("Failed to create image from CGMainDisplayID()")
}
