import SwiftUI

// MARK: - PairingView
// QR-based desktop pairing. Desktop shows QR; user scans here.

struct PairingView: View {
    @StateObject private var pairing = PairingService.shared
    @Environment(\.dismiss) private var dismiss

    @State private var phase: Phase = .intro
    @State private var error: String?
    @State private var success = false

    enum Phase { case intro, scanning, paired }

    var body: some View {
        ZStack {
            Color.recallBackground.ignoresSafeArea()

            // Ambient glow
            RadialGradient(
                colors: [Color.recallPrimary.opacity(0.06), .clear],
                center: .top,
                startRadius: 0,
                endRadius: 350
            )
            .ignoresSafeArea()

            VStack(spacing: 0) {
                // Nav bar
                HStack {
                    Button { dismiss() } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 16, weight: .light))
                            .foregroundColor(.recallOnSurfaceVariant.opacity(0.6))
                    }
                    Spacer()
                }
                .padding(.horizontal, 24)
                .padding(.top, 20)
                .padding(.bottom, 8)

                switch phase {
                case .intro:   introContent
                case .scanning: scanContent
                case .paired:  pairedContent
                }

                Spacer()
            }
        }
        .preferredColorScheme(.dark)
    }

    // MARK: - Intro

    private var introContent: some View {
        VStack(spacing: 40) {
            Spacer()

            VStack(spacing: 16) {
                ZStack {
                    Circle()
                        .fill(Color.recallPrimary.opacity(0.08))
                        .frame(width: 80, height: 80)

                    Image(systemName: "qrcode.viewfinder")
                        .font(.system(size: 34, weight: .ultraLight))
                        .foregroundColor(.recallPrimary.opacity(0.7))
                }

                VStack(spacing: 8) {
                    Text("Pair with Desktop")
                        .font(.system(size: 28, weight: .light))
                        .foregroundColor(.recallOnSurface)

                    Text("Open Recall on your Mac or PC, go to Settings → Pair Phone, then scan the QR code.")
                        .font(.recallBody(14))
                        .foregroundColor(.recallOnSurfaceVariant.opacity(0.55))
                        .multilineTextAlignment(.center)
                        .lineSpacing(4)
                        .padding(.horizontal, 16)
                }
            }

            // Privacy note
            HStack(spacing: 8) {
                Image(systemName: "lock.fill")
                    .font(.system(size: 11, weight: .light))
                    .foregroundColor(.recallPrimary.opacity(0.5))
                Text("Credentials stored in Keychain. Never leave your device.")
                    .font(.recallLabel(10))
                    .foregroundColor(.recallOnSurfaceVariant.opacity(0.4))
                    .kerning(0.5)
            }
            .padding(.horizontal, 32)

            Spacer()

            primaryButton("Scan QR Code") {
                phase = .scanning
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 48)
        }
    }

    // MARK: - Scanning

    private var scanContent: some View {
        VStack(spacing: 24) {
            Text("Scan the QR on your desktop")
                .font(.recallBody(14))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.5))
                .padding(.top, 24)

            // Camera viewfinder
            ZStack {
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(Color.recallSurfaceLow)
                    .frame(height: 320)
                    .padding(.horizontal, 24)

                QRScannerView { raw in
                    handleQR(raw)
                }
                .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
                .frame(height: 300)
                .padding(.horizontal, 36)

                // Corner brackets
                ScannerBrackets()
                    .frame(height: 320)
                    .padding(.horizontal, 24)
            }

            if let error {
                Text(error)
                    .font(.recallBody(13))
                    .foregroundColor(.red.opacity(0.7))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)
            }

            Button("Cancel") { phase = .intro }
                .font(.recallBody(14))
                .foregroundColor(.recallOnSurfaceVariant.opacity(0.4))
                .padding(.top, 8)
        }
    }

    // MARK: - Paired success

    private var pairedContent: some View {
        VStack(spacing: 40) {
            Spacer()

            VStack(spacing: 16) {
                ZStack {
                    Circle()
                        .fill(Color.recallPrimary.opacity(0.08))
                        .frame(width: 80, height: 80)
                        .overlay(
                            Circle()
                                .strokeBorder(Color.recallPrimary.opacity(0.2), lineWidth: 1)
                        )

                    Image(systemName: "checkmark")
                        .font(.system(size: 28, weight: .light))
                        .foregroundColor(.recallPrimary)
                }
                .transition(.scale.combined(with: .opacity))

                VStack(spacing: 8) {
                    Text("Paired")
                        .font(.system(size: 28, weight: .light))
                        .foregroundColor(.recallOnSurface)

                    if let id = pairing.pairedDeviceID {
                        Text(id)
                            .font(.recallLabel(10))
                            .foregroundColor(.recallOnSurfaceVariant.opacity(0.35))
                            .kerning(1)
                    }
                }
            }
            .animation(.spring(response: 0.5, dampingFraction: 0.7), value: success)

            Spacer()

            primaryButton("Done") { dismiss() }
                .padding(.horizontal, 24)
                .padding(.bottom, 48)
        }
    }

    // MARK: - Helpers

    private func handleQR(_ raw: String) {
        error = nil
        do {
            let config = try PairingService.shared.parsePairingQR(raw)
            withAnimation(.spring(response: 0.4, dampingFraction: 0.75)) {
                PairingService.shared.pair(with: config)
                success = true
                phase = .paired
            }
        } catch {
            withAnimation { self.error = error.localizedDescription }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                withAnimation { self.error = nil }
            }
        }
    }

    @ViewBuilder
    private func primaryButton(_ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Text(label.uppercased())
                    .font(.recallLabel(10))
                    .foregroundColor(.recallOnPrimary)
                    .kerning(2.5)
                Image(systemName: "arrow.right")
                    .font(.system(size: 12, weight: .light))
                    .foregroundColor(.recallOnPrimary)
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
        }
        .buttonStyle(ScaleButtonStyle(scale: 0.97))
    }
}

// MARK: - Viewfinder brackets

private struct ScannerBrackets: View {
    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height
            let inset: CGFloat = 32
            let len: CGFloat = 28
            let thick: CGFloat = 2.5

            ZStack {
                // top-left
                bracket(x: inset, y: inset, len: len, thick: thick, rotation: 0)
                // top-right
                bracket(x: w - inset, y: inset, len: len, thick: thick, rotation: 90)
                // bottom-right
                bracket(x: w - inset, y: h - inset, len: len, thick: thick, rotation: 180)
                // bottom-left
                bracket(x: inset, y: h - inset, len: len, thick: thick, rotation: 270)
            }
        }
    }

    private func bracket(x: CGFloat, y: CGFloat, len: CGFloat, thick: CGFloat, rotation: Double) -> some View {
        BracketShape(len: len, thick: thick)
            .fill(Color.recallPrimary.opacity(0.6))
            .frame(width: len, height: len)
            .rotationEffect(.degrees(rotation))
            .position(x: x, y: y)
    }
}

private struct BracketShape: Shape {
    let len: CGFloat
    let thick: CGFloat

    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: 0, y: len))
        path.addLine(to: CGPoint(x: 0, y: 0))
        path.addLine(to: CGPoint(x: len, y: 0))
        return path.strokedPath(StrokeStyle(lineWidth: thick, lineCap: .round))
    }
}
