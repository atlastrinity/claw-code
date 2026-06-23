//
//  ClawControllerApp.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI
import ComposableArchitecture

@main
struct ClawControllerApp: App {
    var body: some Scene {
        WindowGroup {
            ChatView(
                store: Store(
                    initialState: ChatState(),
                    reducer: chatReducer,
                    environment: .live
                )
            )
        }
    }
}
