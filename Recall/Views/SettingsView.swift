import SwiftUI

// Screen 6: Settings
// Matches settings_magical_refinement design exactly:
// privacy pillar, appearance, extensions (coming soon), archive

struct SettingsView: View {
    @StateObject private var vm = SettingsViewModel()
    @State private var showPairing = false

    var body: some View {
        NavigationStack {
            ZStack(alignment: .top) {
                Color.recallBackground.ignoresSafeArea()

                // Subtle top glow
                RadialGradient(
                    colors: [Color.recallPrimary.opacity(0.03), .clear],
                    center: UnitPoint(x: 0.5, y: 0),
                    startRadius: 0,
                    endRadius: 300
                )
                .ignoresSafeArea()

                ScrollView(showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 32) {

                        Color.clear.frame(height: 64)
                            .padding(.bottom, 8)

                        // Privacy pillar
                        privacyPillar

                        // Device pairing section
                        settingsSection("DESKTOP PAIRING") {
                            pairingRow
                        }

                        // Transfer section
                        settingsSection("TRANSFER") {
                            autoTransferRow
                            if vm.pairing.isPaired {
                                sendAllRow
                            }
                        }

                        // Appearance
                        settingsSection("APPEARANCE") {
                            settingsRow(icon: "moon.fill", label: "Dark Mode") {
                                HStack(spacing: 8) {
                                    Text("ALWAYS")
                                        .font(.recallLabel(9))
                                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.3))
                                        .kerning(1.5)
                                    togglePill(isOn: .constant(true))
                                }
                            }
                        }

                        // Extensions (coming soon)
                        settingsSection("EXTENSIONS") {
                            comingSoonRow(icon: "sparkles", label: "Intelligent Analysis", badge: "Soon")
                            comingSoonRow(icon: "lock.shield", label: "E2EE Sync", badge: "Planned")
                        }

                        // Archive
                        settingsSection("ARCHIVE") {
                            settingsNavRow(icon: "cylinder", label: "Storage Management")
                            settingsNavRow(icon: "square.and.arrow.up", label: "Export Data")
                        }

                        // Footer
                        Text("RECALL V1.0 · BUILT FOR FOCUS AND MEMORY")
                            .font(.recallLabel(9))
                            .foregroundColor(.recallOnSurfaceVariant.opacity(0.25))
                            .kerning(2)
                            .frame(maxWidth: .infinity)
                            .padding(.top, 32)
                            .padding(.bottom, 140)
                    }
                    .padding(.horizontal, 24)
                }

                RecallFloatingHeader()
            }
            .navigationBarHidden(true)
            .sheet(isPresented: $showPairing) { PairingView() }
        }
        .preferredColorScheme(.dark)
    }

    // MARK: - Privacy Pillar

    private var privacyPillar: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 16) {
                ZStack {
                    Circle()
                        .fill(Color.recallPrimary.opacity(0.1))
                        .frame(width: 44, height: 44)
                        .overlay(
                            Circle()
                                .strokeBorder(Color.recallPrimary.opacity(0.2), lineWidth: 1)
                        )
                        .shadow(color: Color.recallPrimary.opacity(0.25), radius: 10)

                    Image(systemName: "lock.shield.fill")
                        .font(.system(size: 18, weight: .light))
                        .foregroundColor(.recallPrimary)
                }

                VStack(alignment: .leading, spacing: 3) {
                    Text("Privacy First")
                        .font(.recallBody(14))
                        .foregroundColor(.recallOnSurface)

                    Text("DEVICE LOCAL ENCRYPTION")
                        .font(.recallLabel(8))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.35))
                        .kerning(2)
                }
            }

            Text("Everything stays on your device. No cloud. No tracking. Transfer is optional and entirely under your control.")
                .font(.recallBody(13))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.7))
                .lineSpacing(4)
        }
        .padding(20)
        .glassPanel()
    }

    // MARK: - Pairing Row

    private var pairingRow: some View {
        Group {
            if vm.pairing.isPaired {
                VStack(spacing: 0) {
                    settingsRow(icon: "checkmark.circle", label: "Paired") {
                        if let id = vm.pairing.pairedDeviceID {
                            Text(truncate(id, to: 18))
                                .font(.recallLabel(9))
                                .foregroundColor(.recallPrimary.opacity(0.5))
                                .kerning(0.5)
                        }
                    }

                    Button {
                        withAnimation { vm.unpair() }
                    } label: {
                        HStack(spacing: 8) {
                            Image(systemName: "link.badge.minus")
                                .font(.system(size: 14, weight: .light))
                            Text("Unpair Device")
                                .font(.recallBody(13))
                        }
                        .foregroundColor(.red.opacity(0.65))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.vertical, 14)
                        .padding(.horizontal, 4)
                    }
                    .buttonStyle(ScaleButtonStyle(scale: 0.97))
                }
            } else {
                Button { showPairing = true } label: {
                    settingsRow(icon: "qrcode", label: "Pair with Desktop") {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 12, weight: .light))
                            .foregroundColor(.recallOnSurfaceVariant.opacity(0.2))
                    }
                }
                .buttonStyle(ScaleButtonStyle(scale: 0.98))
            }
        }
    }

    // MARK: - Auto Transfer Row

    private var autoTransferRow: some View {
        settingsRow(icon: "arrow.up.circle", label: "Auto-Transfer") {
            togglePill(isOn: $vm.autoTransferEnabled)
        }
        .opacity(vm.pairing.isPaired ? 1 : 0.4)
        .disabled(!vm.pairing.isPaired)
    }

    // MARK: - Send All Row

    private var sendAllRow: some View {
        Button { vm.sendAllPending() } label: {
            settingsRow(icon: "arrow.triangle.2.circlepath", label: "Send Pending Memories") {
                if vm.isSyncing {
                    ProgressView()
                        .tint(.recallPrimary.opacity(0.6))
                        .scaleEffect(0.7)
                } else if !vm.lastSyncMessage.isEmpty {
                    Text(vm.lastSyncMessage)
                        .font(.recallLabel(9))
                        .foregroundColor(.recallPrimary.opacity(0.5))
                        .lineLimit(1)
                }
            }
        }
        .buttonStyle(ScaleButtonStyle(scale: 0.97))
        .disabled(vm.isSyncing)
    }

    // MARK: - Builder helpers

    @ViewBuilder
    private func settingsSection<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.recallLabel(9))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.3))
                .kerning(3)
                .padding(.leading, 4)

            VStack(spacing: 4) {
                content()
            }
        }
    }

    @ViewBuilder
    private func settingsRow<Trailing: View>(
        icon: String,
        label: String,
        @ViewBuilder trailing: () -> Trailing
    ) -> some View {
        HStack(spacing: 16) {
            Image(systemName: icon)
                .font(.system(size: 17, weight: .light))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.4))
                .frame(width: 22)

            Text(label)
                .font(.recallBody(14))
                .foregroundColor(.recallOnSurface.opacity(0.85))

            Spacer()

            trailing()
        }
        .padding(.vertical, 14)
        .padding(.horizontal, 4)
        .background(Color.white.opacity(0.001)) // tap area
        .contentShape(Rectangle())
    }

    private func settingsNavRow(icon: String, label: String) -> some View {
        Button {} label: {
            settingsRow(icon: icon, label: label) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .light))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.2))
            }
        }
        .buttonStyle(ScaleButtonStyle(scale: 0.98))
    }

    private func comingSoonRow(icon: String, label: String, badge: String) -> some View {
        settingsRow(icon: icon, label: label) {
            Text(badge.uppercased())
                .font(.recallLabel(8))
                .foregroundColor(.recallPrimary.opacity(0.5))
                .kerning(1.5)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.recallPrimary.opacity(0.06))
                .clipShape(Capsule())
                .overlay(Capsule().strokeBorder(Color.recallPrimary.opacity(0.1), lineWidth: 0.5))
        }
        .opacity(0.5)
        .grayscale(0.3)
    }

    private func togglePill(isOn: Binding<Bool>) -> some View {
        Toggle("", isOn: isOn)
            .toggleStyle(RecallToggleStyle())
    }

    private func truncate(_ string: String, to length: Int) -> String {
        string.count > length ? String(string.prefix(length)) + "…" : string
    }
}

// MARK: - Custom Toggle

private struct RecallToggleStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        ZStack(alignment: configuration.isOn ? .trailing : .leading) {
            Capsule()
                .fill(configuration.isOn ? Color.recallPrimary.opacity(0.7) : Color.white.opacity(0.1))
                .frame(width: 36, height: 20)

            Circle()
                .fill(configuration.isOn ? Color(hex: "#001a41") : Color.white)
                .frame(width: 14, height: 14)
                .padding(3)
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.75), value: configuration.isOn)
        .onTapGesture { configuration.isOn.toggle() }
    }
}
