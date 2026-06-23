//
//  LaunchScreen.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI

struct LaunchScreen: View {
    var body: some View {
        ZStack {
            Color.background.ignoresSafeArea()

            VStack {
                Image(systemName: "bolt.fill")
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: 80, height: 80)
                    .foregroundStyle(
                        LinearGradient(
                            colors: [.primary, .secondary],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                    .padding(.bottom, 20)

                Text("Claw Controller")
                    .font(.title2)
                    .fontWeight(.bold)
                    .foregroundColor(.textPrimary)
            }
        }
    }
}

#Preview {
    LaunchScreen()
}
