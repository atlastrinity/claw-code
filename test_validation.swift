import SwiftUI

struct ContentView: View {
    @State private var text = ""
    @State private var count = 0
    
    var body: some View {
        VStack {
            TextField("Введіть текст", text: $text)
                .padding()
            
            Button("Натисни мене") {
                count += 1
                print("Клік: \(count)")
            }
            .padding()
            
            Text("Кількість кліків: \(count)")
                .padding()
        }
    }
}

struct ContentView_Previews: PreviewProvider {
    static var previews: some View {
        ContentView()
    }
}
