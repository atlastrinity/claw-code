//
//  ToolsReducer.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import Foundation
import ComposableArchitecture

// MARK: - Reducer

@Reducer
struct ToolsReducer {
    @ObservableState
    struct State: Equatable {
        var toolCalls: [ToolCall] = []
        var selectedToolId: String?
        var filter: ToolFilter = .all
        var sortBy: ToolSort = .startTime
        var searchQuery: String = ""
        var stats: ToolStats = .empty
        var isExpanded: Set<String> = []

        static let empty = Self()
    }

    enum Action: Equatable {
        case loadTools
        case toolsLoaded([ToolCall])
        case selectTool(String?)
        case toggleExpand(String)
        case setFilter(ToolFilter)
        case setSort(ToolSort)
        case setSearchQuery(String)
        case updateStats(ToolStats)
    }

    enum Mutation {
        case setToolCalls([ToolCall])
        case setSelectedTool(String?)
        case toggleExpand(String)
        case setFilter(ToolFilter)
        case setSort(ToolSort)
        case setSearchQuery(String)
        case updateStats(ToolStats)
    }

    var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case .loadTools:
                return .none

            case .toolsLoaded(let calls):
                return .merge(
                    .send(.updateStats(.calculate(from: calls))),
                    .send(.setFilter(.all))
                )

            case .selectTool(let id):
                state.selectedToolId = id
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

            case .setSearchQuery(let query):
                state.searchQuery = query
                return .none

            case .updateStats(let stats):
                state.stats = stats
                return .none
            }
        }
    }
}

// MARK: - Supporting Types

struct ToolStats {
    var total: Int = 0
    var success: Int = 0
    var failed: Int = 0
    var totalDuration: TimeInterval = 0
    var avgDuration: TimeInterval = 0
    var toolUsage: [ToolName: Int] = [:]

    var completionRate: Double {
        guard total > 0 else { return 0 }
        return Double(success) / Double(total)
    }

    static let empty = Self()
}

enum ToolFilter {
    case all
    case success
    case failed
    case byTool(ToolName)
    case byDuration(DurationRange)

    var title: String {
        switch self {
        case .all:
            return "All"
        case .success:
            return "Success"
        case .failed:
            return "Failed"
        case .byTool(let name):
            return name.rawValue
        case .byDuration(let range):
            return range.title
        }
    }
}

enum ToolSort {
    case startTime
    case duration
    case name
    case status

    var title: String {
        switch self {
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

enum DurationRange: String, CaseIterable {
    case fast = "< 100ms"
    case normal = "100ms - 2s"
    case slow = "> 2s"

    var title: String {
        return rawValue
    }

    var predicate: (TimeInterval) -> Bool {
        switch self {
        case .fast:
            return $0 < 0.1
        case .normal:
            return $0 >= 0.1 && $0 <= 2.0
        case .slow:
            return $0 > 2.0
        }
    }
}
