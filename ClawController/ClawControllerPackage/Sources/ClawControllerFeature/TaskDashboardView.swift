import SwiftUI
import ClawControllerFeature

public struct TaskDashboardView: View {
    @State private var taskNodes: [TaskNode] = []
    
    public var body: some View {
        HStack {
            List(taskNodes) { node in
                TaskRow(node: node)
            }
        }
        .navigationTitle("Task Roadmap")
    }
}

struct TaskRow: View {
    let node: TaskNode
    
    var body: some View {
        HStack {
            Text(node.content)
            Spacer()
            statusIndicator
        }
    }
    
    @ViewBuilder
    private var statusIndicator: some View {
        switch node.status {
        case .pending:
            Image(systemName: "circle")
        case .in_progress:
            Image(systemName: "arrow.clockwise.circle")
        case .completed:
            Image(systemName: "checkmark.circle.fill").foregroundColor(.green)
        case .failed:
            Image(systemName: "xmark.circle.fill").foregroundColor(.red)
        }
    }
}
