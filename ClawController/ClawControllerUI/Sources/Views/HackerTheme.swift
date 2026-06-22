import SwiftUI

enum HackerTheme {
    static let backgroundColor = Color(red: 0.05, green: 0.05, blue: 0.05)
    static let accentColor = Color.green
    static let terminalFont = Font.system(.body, design: .monospaced)
    static let panelBorderColor = Color.green.opacity(0.3)
    
    static func styledView<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        content()
            .padding()
            .background(backgroundColor)
            .foregroundColor(accentColor)
            .font(terminalFont)
            .cornerRadius(0)
            .overlay(
                RoundedRectangle(cornerRadius: 0)
                    .stroke(panelBorderColor, lineWidth: 1)
            )
    }
}
