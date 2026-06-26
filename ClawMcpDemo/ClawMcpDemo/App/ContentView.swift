//
//  ContentView.swift
//  ClawMcpDemo
//
//  Created by test
//

import SwiftUI

struct ContentView: View {
    @State private var count = 0

    var body: some View {
        VStack(spacing: 20) {
            Text("Counter App")
                .font(.largeTitle)
                .fontWeight(.bold)

            Text("Count: \(count)")
                .font(.title)
                .foregroundColor(count < 0 ? .red : .primary)

            HStack(spacing: 20) {
                Button(action: {
                    count -= 1
                }) {
                    Text("-")
                        .font(.title)
                        .fontWeight(.semibold)
                        .frame(width: 60, height: 60)
                        .background(Color.blue)
                        .foregroundColor(.white)
                        .cornerRadius(30)
                }

                Button(action: {
                    count += 1
                }) {
                    Text("+")
                        .font(.title)
                        .fontWeight(.semibold)
                        .frame(width: 60, height: 60)
                        .background(Color.blue)
                        .foregroundColor(.white)
                        .cornerRadius(30)
                }
            }

            Text("Tap buttons to change count")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .padding()
    }
}

#Preview {
    ContentView()
}
