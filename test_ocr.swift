import Vision
import CoreGraphics
import Foundation

print("Creating image...")
let colorSpace = CGColorSpaceCreateDeviceRGB()
let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
let ctx = CGContext(data: nil, width: 100, height: 100, bitsPerComponent: 8, bytesPerRow: 0, space: colorSpace, bitmapInfo: bitmapInfo.rawValue)!
ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
ctx.fill(CGRect(x: 0, y: 0, width: 100, height: 100))
let img = ctx.makeImage()!

print("Starting OCR request...")
let request = VNRecognizeTextRequest { (request, error) in
    print("OCR Done")
}
let handler = VNImageRequestHandler(cgImage: img, options: [:])
do {
    try handler.perform([request])
    print("Handler perform returned")
} catch {
    print("Error")
}
