//
//  ToolsInspectorView.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI
import ComposableArchitecture

struct ToolsInspectorView: View {
    @Bindable var store: StoreOf<ToolsReducer>

    var body: some View {
        ZStack {
            Color.background.ignoresSafeArea()

            VStack(spacing: 0) {
                // Header Stats
                ToolsStatsHeader(stats: store.stats)
                    .padding(.horizontal)
                    .padding(.top)

                // Filter and Sort Bar
                filterSortBar

                // Timeline View
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(filteredTools) { toolCall in
                            ToolCallCard(
                                toolCall: toolCall,
                                isExpanded: store.isExpanded.contains(toolCall.id),
                                onTap: { store.send(.toggleExpand(toolCall.id)) },
                                onToolTap: { store.send(.selectTool(toolCall.id)) }
                            )
                        }
                    }
                    .padding(.horizontal)
                    .padding(.bottom)
                }
            }
        }
    }

    // MARK: - Computed Properties

    private var filteredTools: [ToolCall] {
        var tools = store.toolCalls

        // Apply filter
        switch store.filter {
        case .all:
            break
        case .success:
            tools = tools.filter { !$0.isError }
        case .failed:
            tools = tools.filter { $0.isError }
        case .byTool(let name):
            tools = tools.filter { $0.name == name }
        case .byDuration(let range):
            tools = tools.filter { range.predicate(toolCall.duration ?? 0) }
        }

        // Apply search query
        if !store.searchQuery.isEmpty {
            let query = store.searchQuery.lowercased()
            tools = tools.filter {
                $0.name.rawValue.lowercased().contains(query) ||
                $0.outputPreview?.lowercased().contains(query) == true
            }
        }

        // Apply sort
        switch store.sortBy {
        case .startTime:
            tools.sort { $0.startedAt > $1.startedAt }
        case .duration:
            tools.sort { ($0.duration ?? 0) > ($1.duration ?? 0) }
        case .name:
            tools.sort { $0.name.rawValue < $1.name.rawValue }
        case .status:
            tools.sort { $0.isError == $1.isError }
        }

        return tools
    }

    private var filterSortBar: some View {
        HStack(spacing: 12) {
            // Filter Chips
            ForEach(availableFilters, id: \.self) { filter in
                FilterChip(
                    title: filter.title,
                    isSelected: isFilterActive(filter),
                    onTap: { store.send(.setFilter(filter)) }
                )
            }
            .padding(.horizontal)

            Spacer()

            // Search Bar
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundColor(.textSecondary)
                TextField("Search tools...", text: $store.searchQuery)
                    .textFieldStyle(.plain)
                    .font(.caption)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(Color.surfaceElevated)
            .cornerRadius(10)
            .frame(width: 150)

            // Sort Button
            Button(action: {
                store.send(.setSort(store.sortBy.next))
            }) {
                HStack(spacing: 4) {
                    Text(sortTitle)
                        .font(.caption)
                    Image(systemName: "chevron.down")
                        .font(.caption2)
                }
                .foregroundColor(.textPrimary)
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(Color.surfaceElevated)
                .cornerRadius(8)
            }
        }
        .padding(.vertical, 8)
        .background(Color.surfaceElevated)
    }

    private var availableFilters: [ToolFilter] {
        var filters: [ToolFilter] = [.all, .success, .failed]

        // Add tool-specific filters
        let uniqueTools = Set(store.toolCalls.map { $0.name })
        for tool in uniqueTools {
            filters.append(.byTool(tool))
        }

        // Add duration filters
        filters.append(.byDuration(.fast))
        filters.append(.byDuration(.normal))
        filters.append(.byDuration(.slow))

        return filters
    }

    private func isFilterActive(_ filter: ToolFilter) -> Bool {
        switch (store.filter, filter) {
        case (.all, .all),
             (.success, .success),
             (.failed, .failed):
            return true
        case (.byTool(let name), .byTool(let other)):
            return name == other
        case (.byDuration, .byDuration):
            return true
        default:
            return false
        }
    }

    private var sortTitle: String {
        switch store.sortBy {
        case .startTime:
            return "Time"
        case .duration:
            return "Duration"
        case .name:
            return "Name"
        case .status:
            return "Status"
        }
    }
}

// MARK: - Supporting Views

struct FilterChip: View {
    let title: String
    let isSelected: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            Text(title)
                .font(.caption)
                .foregroundColor(isSelected ? .white : .textSecondary)
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(
                    isSelected ?
                        LinearGradient(
                            colors: [.primary, .primaryGradient.1],
                            startPoint: .leading,
                            endPoint: .trailing
                        ) :
                        Color.surfaceElevated
                )
                .cornerRadius(8)
        }
    }
}

// MARK: - Preview

#Preview {
    ToolsInspectorView(
        store: Store(
            initialState: ToolsReducer.State(
                toolCalls: [
                    ToolCall(
                        id: "1",
                        name: .readFile,
                        arguments: ["file": "/path/to/file.txt"],
                        startedAt: Date().addingTimeInterval(-3600),
                        duration: 150,
                        isError: false
                    ),
                    ToolCall(
                        id: "2",
                        name: .writeFile,
                        arguments: ["file": "/path/to/output.txt", "content": "test"],
                        startedAt: Date().addingTimeInterval(-3000),
                        duration: 50,
                        isError: false
                    ),
                    ToolCall(
                        id: "3",
                        name: .grepWorkspace,
                        arguments: ["pattern": "test"],
                        startedAt: Date().addingTimeInterval(-60),
                        duration: 2500,
                        isError: true,
                        outputPreview: "Error: pattern not found"
                    ),
                    ToolCall(
                        id: "4",
                        name: .listDir,
                        arguments: ["path": "/"],
                        startedAt: Date(),
                        duration: 10,
                        isError: false
                    )
                ]
            ),
            reducer: ToolsReducer()
        )
    )
}
