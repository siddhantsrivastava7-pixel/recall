import SwiftUI

struct ContentView: View {
    @State private var selectedTab: RecallTab = .home

    var body: some View {
        ZStack(alignment: .bottom) {
            // Tab content
            Group {
                switch selectedTab {
                case .home:     HomeView()
                case .library:  LibraryView()
                case .capture:  HomeView() // placeholder — main capture is via Share Extension
                case .settings: SettingsView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            // Floating bottom nav
            BottomNavView(selectedTab: $selectedTab)
                .padding(.bottom, 24)
                .ignoresSafeArea(.keyboard)
        }
        .ignoresSafeArea(edges: .bottom)
        .preferredColorScheme(.dark)
        .onAppear {
            configureNavBarAppearance()
        }
    }

    private func configureNavBarAppearance() {
        let appearance = UINavigationBarAppearance()
        appearance.configureWithTransparentBackground()
        appearance.backgroundColor = .clear
        UINavigationBar.appearance().standardAppearance = appearance
        UINavigationBar.appearance().scrollEdgeAppearance = appearance
    }
}
