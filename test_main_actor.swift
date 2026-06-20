import Foundation
print("Start")
await Task { @MainActor in
    print("Inside MainActor")
}.value
print("End")
