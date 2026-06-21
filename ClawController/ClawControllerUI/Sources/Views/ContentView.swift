//
//  ContentView.swift
//  ClawControllerUI
//
//  Main view for remote control interface
//

import SwiftUI

struct ContentView: View {
    @State private var state = RemoteControllerState()
    @State private var selectedTab: Tab = .dashboard

    init() {}

    var body: some View {
        NavigationView {
            mainContent
                .navigationTitle("ClawController")
                .toolbar {
                    ToolbarItem(placement: .navigationBarTrailing) {
                        settingsButton
                    }
                }
        }
    }

    // MARK: - Main Content

    private var mainContent: some View {
        TabView(selection: $selectedTab) {
            ConnectionStatusView(state: state)
                .tabItem {
                    Label("Dashboard", systemImage: "chart.bar")
                }
                .tag(Tab.dashboard)

            CommandPanelView(state: state)
                .tabItem {
                    Label("Command", systemImage: "command")
                }
                .tag(Tab.command)

            CommandHistoryView(state: state)
                .tabItem {
                    Label("History", systemImage: "clock.arrow.circlepath")
                }
                .tag(Tab.history)

            SettingsView(state: state)
                .tabItem {
                    Label("Settings", systemImage: "gear")
                }
                .tag(Tab.settings)
        }
        .accentColor(.blue)
    }

    // MARK: - Settings Button

    private var settingsButton: some View {
        Button(action: {
            selectedTab = .settings
        }) {
            Image(systemName: "gear")
        }
        .disabled(!state.isConnected)
    }

    // MARK: - Tab Enum

    enum Tab: String, CaseIterable {
        case dashboard = "Dashboard"
        case command = "Command"
        case history = "History"
        case settings = "Settings"
    }
}

// MARK: - Preview

struct ContentView_Previews: PreviewProvider {
    static var previews: some View {
        ContentView()
            .preferredColorScheme(.dark)
    }
}
