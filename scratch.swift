import Foundation
import CoreGraphics
import AppKit

func resizeImage(image: CGImage, maxDimension: CGFloat) -> CGImage? {
    let width = CGFloat(image.width)
    let height = CGFloat(image.height)
    if width <= maxDimension && height <= maxDimension { return image }
    
    let scale = maxDimension / max(width, height)
    let newWidth = Int(width * scale)
    let newHeight = Int(height * scale)
    
    let colorSpace = CGColorSpaceCreateDeviceRGB()
    let bitmapInfo = CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
    
    guard let context = CGContext(data: nil, width: newWidth, height: newHeight, bitsPerComponent: 8, bytesPerRow: 0, space: colorSpace, bitmapInfo: bitmapInfo) else {
        return nil
    }
    
    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: newWidth, height: newHeight))
    return context.makeImage()
}
print("ok")
