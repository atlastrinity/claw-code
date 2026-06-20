import CoreGraphics
print("capturing")
let img = CGDisplayCreateImage(CGMainDisplayID())
print(img != nil ? "success" : "failed")
