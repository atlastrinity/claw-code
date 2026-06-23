import SwiftUI
import ClawControllerFeature

public struct LogFilterView: View {
    @State private var filterText: String = ""
    @State private var selectedLevel: LogLevel = .all
    
    public enum LogLevel: String, CaseIterable {
        case all = "All"
        case error = "Error"
        case tool = "Tool Call"
    }
    
    public var body: some View {
        VStack {
            Picker("Level", selection: $selectedLevel) {
                ForEach(LogLevel.allCases, id: \.self) { level in
                    Text(level.rawValue).tag(level)
                }
            }
            .pickerStyle(.segmented)
            .padding()
            
            TextField("Filter logs...", text: $filterText)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal)
            
            List {
                // Placeholder for filtered logs list
                Text("Log entries will appear here")
            }
        }
        .navigationTitle("Logs & Filters")
    }
}
