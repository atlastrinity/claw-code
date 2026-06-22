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

    init() {
        UITabBar.appearance().backgroundColor = UIColor(HackerTheme.backgroundColor)
        UITabBar.appearance().unselectedItemTintColor = UIColor.gray
    }

    var body: some View {
        HackerTheme.styledView {
            TabView(selection: $selectedTab) {
                ConnectionStatusView(state: state)
                    .tabItem {
                        Label("Dashboard", systemImage: "terminal")
                    }
                    .tag(Tab.dashboard)

                CommandPanelView(state: state)
                    .tabItem {
                        Label("Command", systemImage: "chevron.right.square")
                    }
                    .tag(Tab.command)

                CommandHistoryView(state: state)
                    .tabItem {
                        Label("History", systemImage: "list.bullet.rectangle")
                    }
                    .tag(Tab.history)

                SettingsView(state: state)
                    .tabItem {
                        Label("Settings", systemImage: "slider.horizontal.3")
                    }
                    .tag(Tab.settings)
            }
            .accentColor(HackerTheme.accentColor)
        }
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
