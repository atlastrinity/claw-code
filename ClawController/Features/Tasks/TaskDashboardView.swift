//
//  TaskDashboardView.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI
import ComposableArchitecture

struct TaskDashboardView: View {
    @Bindable var store: StoreOf<TasksReducer>

    var body: some View {
        ZStack {
            Color.background.ignoresSafeArea()

            VStack(spacing: 0) {
                // Header Stats
                TaskStatsHeader(stats: store.stats)
                    .padding(.horizontal)
                    .padding(.top)

                // Filter and Sort Bar
                filterSortBar

                // Task Tree View
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(filteredTasks) { task in
                            TaskNodeRow(
                                task: task,
                                isExpanded: store.isExpanded.contains(task.id),
                                onTap: { store.send(.toggleExpand(task.id)) },
                                onTaskTap: { store.send(.selectTask(task.id)) }
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

    private var filteredTasks: [TaskNode] {
        let tasks = store.taskNodes

        switch store.filter {
        case .all:
            return tasks
        case .pending:
            return tasks.filter { $0.status == .pending }
        case .inProgress:
            return tasks.filter { $0.status == .inProgress }
        case .completed:
            return tasks.filter { $0.status == .completed }
        case .failed:
            return tasks.filter { $0.status == .failed }
        }
    }

    private var filterSortBar: some View {
        HStack(spacing: 12) {
            // Filter Chips
            ForEach(TaskFilter.allCases, id: \.self) { filter in
                FilterChip(
                    title: filter.title,
                    isSelected: store.filter == filter,
                    onTap: { store.send(.setFilter(filter)) }
                )
            }
            .padding(.horizontal)

            Spacer()

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

    private var sortTitle: String {
        switch store.sortBy {
        case .status:
            return "Status"
        case .title:
            return "Title"
        case .createdAt:
            return "Date"
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
    TaskDashboardView(
        store: Store(
            initialState: TasksReducer.State(
                taskNodes: [
                    TaskNode(
                        id: UUID(),
                        title: "Implement authentication",
                        status: .completed,
                        depth: 0,
                        children: [
                            TaskNode(
                                id: UUID(),
                                title: "Create login screen",
                                status: .completed,
                                depth: 1,
                                children: []
                            ),
                            TaskNode(
                                id: UUID(),
                                title: "Implement token refresh",
                                status: .completed,
                                depth: 1,
                                children: []
                            )
                        ]
                    ),
                    TaskNode(
                        id: UUID(),
                        title: "Build user profile",
                        status: .inProgress,
                        depth: 0,
                        children: [
                            TaskNode(
                                id: UUID(),
                                title: "Create profile form",
                                status: .pending,
                                depth: 1,
                                children: []
                            ),
                            TaskNode(
                                id: UUID(),
                                title: "Implement avatar upload",
                                status: .pending,
                                depth: 1,
                                children: []
                            )
                        ]
                    ),
                    TaskNode(
                        id: UUID(),
                        title: "Write unit tests",
                        status: .pending,
                        depth: 0,
                        children: []
                    )
                ]
            ),
            reducer: TasksReducer()
        )
    )
}
