import SwiftUI

public enum HackerTheme {
    public static let backgroundColor = Color(red: 0.05, green: 0.05, blue: 0.05)
    public static let accentColor = Color.green
    public static let terminalFont = Font.system(.body, design: .monospaced)
    public static let panelBorderColor = Color.green.opacity(0.3)
    
    public static func styledView<Content: View>(@ViewBuilder content: () -> Content) -> some View {
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
