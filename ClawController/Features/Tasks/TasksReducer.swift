//
//  TasksReducer.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation
import ComposableArchitecture

// MARK: - Reducer

@Reducer
struct TasksReducer {
    @ObservableState
    struct State: Equatable {
        var taskNodes: [TaskNode] = []
        var selectedTaskId: UUID?
        var filter: TaskFilter = .all
        var sortBy: TaskSort = .status
        var isExpanded: Set<UUID> = []
        var stats: TaskStats = .empty

        static let empty = Self()
    }

    enum Action: Equatable {
        case loadTasks
        case tasksLoaded([TaskNode])
        case selectTask(UUID?)
        case toggleExpand(UUID)
        case setFilter(TaskFilter)
        case setSort(TaskSort)
        case updateStats(TaskStats)
    }

    enum Mutation {
        case setTaskNodes([TaskNode])
        case setSelectedTask(UUID?)
        case toggleExpand(UUID)
        case setFilter(TaskFilter)
        case setSort(TaskSort)
        case updateStats(TaskStats)
    }

    var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case .loadTasks:
                return .none

            case .tasksLoaded(let nodes):
                return .merge(
                    .send(.updateStats(.calculate(from: nodes))),
                    .send(.setFilter(.all))
                )

            case .selectTask(let id):
                state.selectedTaskId = id
                return .none

            case .toggleExpand(let id):
                if state.isExpanded.contains(id) {
                    state.isExpanded.remove(id)
                } else {
                    state.isExpanded.insert(id)
                }
                return .none

            case .setFilter(let filter):
                state.filter = filter
                return .none

            case .setSort(let sort):
                state.sortBy = sort
                return .none

            case .updateStats(let stats):
                state.stats = stats
                return .none
            }
        }
    }
}

// MARK: - Supporting Types

struct TaskStats {
    var total: Int = 0
    var completed: Int = 0
    var inProgress: Int = 0
    var failed: Int = 0
    var pending: Int = 0

    var completionRate: Double {
        guard total > 0 else { return 0 }
        return Double(completed) / Double(total)
    }

    static let empty = Self()
}

enum TaskFilter {
    case all
    case pending
    case inProgress
    case completed
    case failed
}

enum TaskSort {
    case status
    case title
    case createdAt
}
