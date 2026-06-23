//
//  ColorTokens.swift
//  ClawController
//
//  Created by AI Assistant on 2026-06-23.
//

import SwiftUI

extension Color {
    // Background
    static let background = Color(hex: "0A0A0F")
    static let surface = Color(hex: "12121A")
    static let surfaceElevated = Color(hex: "1A1A28")
    static let card = Color(hex: "1E1E2E")

    // Primary
    static let primary = Color(hex: "7C5CFC")
    static let primaryGradient = LinearGradient(
        colors: [Color(hex: "7C5CFC"), Color(hex: "4F8EFF")],
        startPoint: .topLeading,
        endPoint: .bottomTrailing
    )

    // Secondary
    static let secondary = Color(hex: "00D4AA")

    // Accent
    static let accent = Color(hex: "FF6B9D")

    // Success
    static let success = Color(hex: "00E676")

    // Warning
    static let warning = Color(hex: "FFB74D")

    // Error
    static let error = Color(hex: "FF5252")

    // Text
    static let textPrimary = Color(hex: "EAEAFF")
    static let textSecondary = Color(hex: "8888AA")
    static let textTertiary = Color(hex: "55556A")

    // Code
    static let codeBackground = Color(hex: "0D1117")
}

extension Color {
    init(hex: String) {
        let hex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: hex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch hex.count {
        case 3: // RGB (12-bit)
            (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6: // RGB (24-bit)
            (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8: // ARGB (32-bit)
            (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default:
            (a, r, g, b) = (255, 0, 0, 0)
        }

        self.init(
            .sRGB,
            red: Double(r) / 255,
            green: Double(g) / 255,
            blue: Double(b) / 255,
            opacity: Double(a) / 255
        )
    }
}
