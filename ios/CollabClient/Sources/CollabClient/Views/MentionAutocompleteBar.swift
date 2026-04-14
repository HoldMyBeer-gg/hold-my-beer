import SwiftUI

/// Horizontal scrolling row of @mention suggestion chips.
/// Pass `selectedIndex` to highlight the active suggestion (-1 = no selection).
struct MentionAutocompleteBar: View {
    let matches: [String]
    var selectedIndex: Int = -1
    let onSelect: (String) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(Array(matches.enumerated()), id: \.offset) { idx, name in
                    Button("@\(name)") { onSelect(name) }
                        .font(.caption.bold())
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(idx == selectedIndex
                            ? Color.blue.opacity(0.15)
                            : Color(.secondarySystemBackground))
                        .foregroundStyle(idx == selectedIndex ? .blue : .primary)
                        .clipShape(Capsule())
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
        }
    }
}
