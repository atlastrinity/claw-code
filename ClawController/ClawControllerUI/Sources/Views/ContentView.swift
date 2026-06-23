//
//  ContentView.swift
//  ClawControllerUI
//
//  Main view for remote control interface
//

import SwiftUI
import ClawControllerFeature

struct ContentView: View {
    @State private var state = RemoteControllerState()
    @State private var selectedTab: Tab = .terminal

    init() {
        let appearance = UITabBarAppearance()
        appearance.configureWithOpaqueBackground()
        appearance.backgroundColor = UIColor(HackerTheme.backgroundColor)
        UITabBar.appearance().standardAppearance = appearance
        UITabBar.appearance().scrollEdgeAppearance = appearance
    }

    var body: some View {
        ZStack {
            HackerTheme.backgroundColor.ignoresSafeArea()
            
            TabView(selection: $selectedTab) {
                TerminalView()
                    .tabItem {
                        Label("Terminal", systemImage: "terminal")
                    }
                    .tag(Tab.terminal)

                ChatView()
                    .tabItem {
                        Label("Chat", systemImage: "message")
                    }
                    .tag(Tab.chat)

                Text("Tasks Processor")
                    .tabItem {
                        Label("Tasks", systemImage: "checklist")
                    }
                    .tag(Tab.tasks)
            }
            .accentColor(HackerTheme.accentColor)
        }
    }

    enum Tab: String, CaseIterable {
        case terminal = "Terminal"
        case chat = "Chat"
        case tasks = "Tasks"
    }
}

// MARK: - Preview

struct ContentView_Previews: PreviewProvider {
    static var previews: some View {
        ContentView()
            .preferredColorScheme(.dark)
    }
}
