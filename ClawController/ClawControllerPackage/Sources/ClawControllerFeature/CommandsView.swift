////
//  CommandsView.swift
//  ClawControllerFeature
//
//  View for executing commands on remote system
//

import SwiftUI

struct CommandsView: View {
    @Binding var commandInput: String
    @State private var isExecuting: Bool = false
    @State private var commandHistory: [CommandHistoryEntry] = []
    @State private var showResult: Bool = false
    @State private var resultText: String = ""
    
    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                // Command Input
                VStack(spacing: 12) {
                    Text("Execute Command")
                        .font(.headline)
                    
                    HStack(spacing: 12) {
                        TextField("Enter command...", text: $commandInput)
                            .textFieldStyle(.roundedBorder)
                            .disabled(isExecuting)
                        
                        Button(action: executeCommand) {
                            Label(isExecuting ? "Executing..." : "Send", systemImage: "arrow.up.circle.fill")
                                .frame(width: 44, height: 44)
                                .background(isExecuting ? Color.gray : Color.blue)
                                .foregroundStyle(.white)
                                .cornerRadius(22)
                        }
                        .disabled(commandInput.isEmpty || isExecuting)
                    }
                }
                .padding(.horizontal)
                
                // Command History
                if !commandHistory.isEmpty {
                    VStack(spacing: 12) {
                        HStack {
                            Text("Recent Commands")
                                .font(.headline)
                            
                            Spacer()
                            
                            Button(action: clearHistory) {
                                Label("Clear", systemImage: "trash")
                                    .font(.caption)
                            }
                        }
                        
                        ForEach(commandHistory.prefix(10)) { entry in
                            CommandRow(entry: entry)
                        }
                    }
                    .padding(.horizontal)
                }
                
                Spacer()
            }
            .padding(.vertical, 20)
        }
        .sheet(isPresented: $showResult) {
            CommandResultView(resultText: $resultText)
        }
    }
    
    private func executeCommand() {
        guard !commandInput.isEmpty && !isExecuting else { return }
        
        isExecuting = true
        
        // Simulate command execution
        Task {
            // In real app, call RemoteService.executeCommand()
            try? await Task.sleep(for: .milliseconds(800))
            
            resultText = "Command '\(commandInput)' executed successfully!\n\nResponse: Command processed with status: completed"
            
            isExecuting = false
            commandInput = ""
            
            // Add to history
            let newEntry = CommandHistoryEntry(
                command: commandInput,
                status: .success,
                startTime: Date()
            )
            commandHistory.insert(newEntry, at: 0)
            
            showResult = true
        }
    }
    
    private func clearHistory() {
        commandHistory.removeAll()
    }
}

// MARK: - Command Row

struct CommandRow: View {
    let entry: CommandHistoryEntry
    
    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: statusIcon)
                .foregroundStyle(statusColor)
                .frame(width: 24)
            
            VStack(alignment: .leading, spacing: 4) {
                Text(entry.command)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                
                Text(entry.timestamp)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            
            Spacer()
            
            Text(statusText)
                .font(.caption)
                .fontWeight(.medium)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(statusColor.opacity(0.15))
                .foregroundStyle(statusColor)
                .cornerRadius(6)
        }
        .padding()
        .background(Color(.secondarySystemBackground))
        .cornerRadius(10)
    }
    
    private var statusIcon: String {
        switch entry.status {
        case .executing:
            return "arrow.triangle.2.circlepath"
        case .success:
            return "checkmark.circle.fill"
        case .failed:
            return "xmark.circle.fill"
        case .cancelled:
            return "stop.circle.fill"
        case .pending:
            return "clock"
        }
    }
    
    private var statusColor: Color {
        switch entry.status {
        case .executing:
            return .blue
        case .success:
            return .green
        case .failed:
            return .red
        case .cancelled:
            return .gray
        case .pending:
            return .orange
        }
    }
    
    private var statusText: String {
        entry.status.rawValue.capitalized
    }
}

// MARK: - Command Result View

struct CommandResultView: View {
    @Binding var resultText: String
    
    var body: some View {
        NavigationStack {
            ScrollView {
                Text(resultText)
                    .font(.body)
                    .padding()
            }
            .navigationTitle("Command Result")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Done") {
                        // Close sheet
                    }
                }
            }
        }
    }
}

// MARK: - Preview

#Preview {
    CommandsView(commandInput: .constant(""))
}