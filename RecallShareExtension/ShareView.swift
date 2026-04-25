import SwiftUI

// Screen 4: Moment of Capture
// Matches moment_of_capture_premium design:
// glass overlay over blurred canvas, success state,
// context note field, luxury Done button

struct ShareView: View {
    @StateObject private var vm = ShareViewModel()
    @FocusState private var noteFocused: Bool

    var extensionContext: NSExtensionContext?

    var body: some View {
        ZStack {
            // Blurred canvas background (Archive impression)
            archiveCanvas

            // Full-screen overlay with glass panel
            Color.black.opacity(0.72)
                .ignoresSafeArea()
                .background(.ultraThinMaterial)

            if vm.didSave {
                successView
                    .transition(.scale(scale: 0.95).combined(with: .opacity))
            } else {
                capturePanel
                    .transition(.scale(scale: 0.97).combined(with: .opacity))
            }
        }
        .preferredColorScheme(.dark)
        .task {
            await vm.loadItems(from: extensionContext)
        }
        .animation(.spring(response: 0.45, dampingFraction: 0.75), value: vm.didSave)
    }

    // MARK: - Archive canvas (blurred bg impression)

    private var archiveCanvas: some View {
        VStack(spacing: 16) {
            ForEach(0..<3) { _ in
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(Color.recallSurfaceLow)
                    .frame(height: 120)
            }
        }
        .padding(.horizontal, 24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .scaleEffect(0.9)
        .opacity(0.05)
        .blur(radius: 20)
        .ignoresSafeArea()
    }

    // MARK: - Capture Panel

    private var capturePanel: some View {
        VStack(spacing: 0) {
            Spacer()

            VStack(spacing: 0) {
                // Header with success icon
                VStack(spacing: 16) {
                    ZStack {
                        Circle()
                            .fill(Color.recallPrimary.opacity(0.10))
                            .frame(width: 48, height: 48)

                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 22, weight: .light))
                            .foregroundColor(.recallPrimary)
                    }

                    Text("NOW PART OF YOUR MEMORY")
                        .font(.recallLabel(9))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.65))
                        .kerning(3.5)
                }
                .padding(.top, 36)
                .padding(.bottom, 28)

                // Content preview card
                contentPreview
                    .padding(.horizontal, 28)
                    .padding(.bottom, 32)

                // Note field + actions
                VStack(spacing: 24) {
                    noteField

                    actionArea
                }
                .padding(.horizontal, 28)
                .padding(.bottom, 44)
            }
            .background(
                ZStack {
                    Color.recallSurface.opacity(0.9)
                    LinearGradient(
                        colors: [Color.white.opacity(0.08), Color.white.opacity(0.01)],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                }
                .background(.ultraThinMaterial)
                .clipShape(RoundedRectangle(cornerRadius: 28, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 28, style: .continuous)
                        .strokeBorder(Color.white.opacity(0.05), lineWidth: 0.5)
                )
                .shadow(color: .black.opacity(0.6), radius: 40, x: 0, y: -10)
            )
            .padding(.horizontal, 20)

            Spacer(minLength: 24)
        }
    }

    // MARK: - Content Preview

    private var contentPreview: some View {
        ZStack(alignment: .bottomLeading) {
            // Thumbnail placeholder
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(Color.recallSurfaceLow)
                .aspectRatio(4/3, contentMode: .fit)
                .overlay(
                    ZStack {
                        Image(systemName: "link")
                            .font(.system(size: 36, weight: .ultraLight))
                            .foregroundColor(.recallPrimary.opacity(0.2))

                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.06), lineWidth: 0.5)
                    }
                )

            // Bottom gradient overlay
            LinearGradient(
                colors: [.clear, .black.opacity(0.9)],
                startPoint: UnitPoint(x: 0.5, y: 0.4),
                endPoint: .bottom
            )
            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))

            // Text overlay
            VStack(alignment: .leading, spacing: 4) {
                if !vm.source.isEmpty {
                    Text(vm.source.uppercased())
                        .font(.recallLabel(7))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.45))
                        .kerning(2.5)
                }
                Text(vm.title.isEmpty ? "Saved memory" : vm.title)
                    .font(.system(size: 15, weight: .light))
                    .foregroundColor(.recallOnSurface.opacity(0.95))
                    .lineLimit(2)
            }
            .padding(16)
        }
    }

    // MARK: - Note Field

    private var noteField: some View {
        HStack(spacing: 12) {
            Image(systemName: "note.text")
                .font(.system(size: 14, weight: .light))
                .foregroundColor(noteFocused ? .recallPrimary.opacity(0.7) : .recallPrimary.opacity(0.3))
                .animation(.easeInOut(duration: 0.2), value: noteFocused)

            TextField("", text: $vm.note)
                .placeholder(when: vm.note.isEmpty) {
                    Text("Add context…")
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.25))
                        .font(.recallBody(14))
                }
                .focused($noteFocused)
                .foregroundColor(.recallOnSurface)
                .font(.recallBody(14))
                .tint(.recallPrimary)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
        .background(
            ZStack {
                Color.white.opacity(0.04)
                LinearGradient(
                    colors: [Color.white.opacity(0.06), Color.white.opacity(0.01)],
                    startPoint: .top,
                    endPoint: .bottom
                )
            }
            .clipShape(Capsule())
            .overlay(
                Capsule().strokeBorder(Color.white.opacity(0.06), lineWidth: 0.5)
            )
        )
    }

    // MARK: - Action area

    private var actionArea: some View {
        VStack(spacing: 16) {
            // Primary save button
            Button {
                noteFocused = false
                vm.save()
            } label: {
                HStack(spacing: 12) {
                    if vm.isSaving {
                        ProgressView()
                            .tint(Color(hex: "#002e69"))
                            .scaleEffect(0.8)
                    } else {
                        Text("DONE")
                            .font(.recallLabel(10))
                            .foregroundColor(Color(hex: "#002e69"))
                            .kerning(3)

                        Image(systemName: "arrow.right")
                            .font(.system(size: 12, weight: .regular))
                            .foregroundColor(Color(hex: "#002e69"))
                    }
                }
                .frame(maxWidth: .infinity)
                .frame(height: 52)
                .background(
                    LinearGradient(
                        colors: [Color(hex: "#4b8eff").opacity(0.85), Color(hex: "#3c73d2").opacity(0.9)],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
                .clipShape(Capsule())
                .shadow(color: Color(hex: "#4b8eff").opacity(0.25), radius: 16, x: 0, y: 6)
            }
            .buttonStyle(ScaleButtonStyle(scale: 0.97))
            .disabled(vm.isSaving)

            // Status hint
            let pairedAndAuto = PairingService.shared.isPaired
                && (UserDefaults(suiteName: AppConstants.appGroupID)?.bool(forKey: AppConstants.autoTransferKey) ?? false)

            Text(pairedAndAuto ? "SENT TO YOUR DESKTOP" : "STORED LOCALLY")
                .font(.recallLabel(9))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.28))
                .kerning(2.5)

            // Cancel
            Button {
                extensionContext?.completeRequest(returningItems: nil)
            } label: {
                Text("Cancel")
                    .font(.recallBody(13))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.35))
            }
            .buttonStyle(ScaleButtonStyle(scale: 0.96))
        }
    }

    // MARK: - Success View

    private var successView: some View {
        VStack(spacing: 24) {
            ZStack {
                Circle()
                    .fill(Color.recallPrimary.opacity(0.08))
                    .frame(width: 80, height: 80)

                Image(systemName: "checkmark")
                    .font(.system(size: 32, weight: .ultraLight))
                    .foregroundColor(.recallPrimary)
            }

            Text("Saved")
                .font(.system(size: 26, weight: .light))
                .foregroundColor(.recallOnSurface)
        }
        .onAppear {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                extensionContext?.completeRequest(returningItems: nil)
            }
        }
    }
}
