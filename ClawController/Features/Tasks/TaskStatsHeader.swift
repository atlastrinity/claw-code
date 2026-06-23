//
//  TaskStatsHeader.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI

struct TaskStatsHeader: View {
    let stats: TaskStats

    var body: some View {
        VStack(spacing: 16) {
            // Title
            Text("Task Dashboard")
                .font(.headline)
                .foregroundColor(.textPrimary)
                .frame(maxWidth: .infinity, alignment: .leading)

            // Stats Grid
            LazyVGrid(columns: [
                GridItem(.flexible(), spacing: 12),
                GridItem(.flexible(), spacing: 12),
                GridItem(.flexible(), spacing: 12)
            ], spacing: 12) {
                StatCard(
                    title: "Total",
                    value: stats.total,
                    color: .primary,
                    isProgress: false
                )
                StatCard(
                    title: "Completed",
                    value: stats.completed,
                    color: .secondary,
                    isProgress: false
                )
                StatCard(
                    title: "Failed",
                    value: stats.failed,
                    color: .error,
                    isProgress: false
                )
                StatCard(
                    title: "In Progress",
                    value: stats.inProgress,
                    color: .orange,
                    isProgress: false
                )
                StatCard(
                    title: "Pending",
                    value: stats.pending,
                    color: .textTertiary,
                    isProgress: false
                )
                StatCard(
                    title: "Completion Rate",
                    value: "\(Int(stats.completionRate * 100))%",
                    color: .secondary,
                    isProgress: true,
                    progressValue: stats.completionRate
                )
            }
            .padding(.horizontal)

            // Summary Text
            if stats.total > 0 {
                HStack(spacing: 8) {
                    Image(systemName: "info.circle.fill")
                        .foregroundColor(.textTertiary)
                    Text(summaryText)
                        .font(.caption)
                        .foregroundColor(.textSecondary)
                }
                .padding(.horizontal)
                .padding(.bottom, 8)
            }
        }
        .padding(.vertical)
        .background(Color.surfaceElevated)
        .cornerRadius(16)
        .shadow(color: Color.black.opacity(0.05), radius: 8, x: 0, y: 4)
    }

    private var summaryText: String {
        if stats.completed == stats.total && stats.total > 0 {
            return "All tasks completed successfully! 🎉"
        } else if stats.failed > 0 {
            return "\(stats.failed) task(s) failed. Review and retry."
        } else if stats.inProgress > 0 {
            return "\(stats.inProgress) task(s) in progress"
        } else if stats.pending > 0 {
            return "\(stats.pending) task(s) pending"
        } else {
            return "No tasks to display"
        }
    }
}

// MARK: - Stat Card

struct StatCard: View {
    let title: String
    let value: String
    let color: Color
    let isProgress: Bool
    let progressValue: Double

    var body: some View {
        VStack(spacing: 8) {
            if isProgress {
                // Circular Progress View
                ZStack {
                    Circle()
                        .stroke(color.opacity(0.2), lineWidth: 8)
                        .frame(width: 60, height: 60)

                    Circle()
                        .trim(from: 0, to: progressValue)
                        .stroke(
                            LinearGradient(
                                colors: [color, color.opacity(0.6)],
                                startPoint: .topLeading,
                                endPoint: .bottomTrailing
                            ),
                            style: StrokeStyle(lineWidth: 8, lineCap: .round)
                        )
                        .frame(width: 60, height: 60)
                        .rotationEffect(.degrees(-90))
                        .animation(.spring(response: 0.5, dampingFraction: 0.75), value: progressValue)

                    Text("\(Int(progressValue * 100))%")
                        .font(.caption2)
                        .fontWeight(.semibold)
                        .foregroundColor(.textPrimary)
                }
                .frame(width: 60, height: 60)
            } else {
                // Simple Stat
                VStack(spacing: 4) {
                    Text(value)
                        .font(.title2)
                        .fontWeight(.bold)
                        .foregroundColor(color)

                    Text(title)
                        .font(.caption)
                        .foregroundColor(.textSecondary)
                }
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .background(Color.surface)
        .cornerRadius(12)
        .shadow(color: Color.black.opacity(0.05), radius: 2, x: 0, y: 1)
    }
}

// MARK: - Preview

#Preview("TaskStatsHeader - All Completed") {
    TaskStatsHeader(stats: TaskStats(
        total: 5,
        completed: 5,
        inProgress: 0,
        failed: 0,
        pending: 0
    ))
}

#Preview("TaskStatsHeader - In Progress") {
    TaskStatsHeader(stats: TaskStats(
        total: 10,
        completed: 4,
        inProgress: 2,
        failed: 0,
        pending: 4
    ))
}

#Preview("TaskStatsHeader - With Failures") {
    TaskStatsHeader(stats: TaskStats(
        total: 8,
        completed: 3,
        inProgress: 1,
        failed: 2,
        pending: 2
    ))
}

#Preview("TaskStatsHeader - Empty") {
    TaskStatsHeader(stats: TaskStats.empty)
}
