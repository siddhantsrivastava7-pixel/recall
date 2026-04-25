import SwiftUI
import AVFoundation

// MARK: - QRScannerView
// Wraps AVCaptureSession for QR scanning; calls onFound with raw string.

struct QRScannerView: UIViewRepresentable {
    var onFound: (String) -> Void

    func makeUIView(context: Context) -> ScannerPreviewView {
        let view = ScannerPreviewView()
        view.delegate = context.coordinator
        view.startSession()
        return view
    }

    func updateUIView(_ uiView: ScannerPreviewView, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(onFound: onFound)
    }

    // MARK: Coordinator

    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate, @unchecked Sendable {
        private let onFound: (String) -> Void
        private var hasFound = false

        init(onFound: @escaping (String) -> Void) {
            self.onFound = onFound
        }

        func metadataOutput(
            _ output: AVCaptureMetadataOutput,
            didOutput objects: [AVMetadataObject],
            from connection: AVCaptureConnection
        ) {
            guard !hasFound,
                  let object = objects.first as? AVMetadataMachineReadableCodeObject,
                  let string = object.stringValue else { return }
            hasFound = true
            DispatchQueue.main.async { [weak self] in self?.onFound(string) }
        }
    }
}

// MARK: - UIKit Preview View

final class ScannerPreviewView: UIView {
    weak var delegate: AVCaptureMetadataOutputObjectsDelegate?

    private let session = AVCaptureSession()
    private var previewLayer: AVCaptureVideoPreviewLayer?

    func startSession() {
        guard AVCaptureDevice.authorizationStatus(for: .video) != .denied else { return }

        AVCaptureDevice.requestAccess(for: .video) { [weak self] granted in
            guard granted, let self else { return }
            DispatchQueue.global(qos: .userInitiated).async { self.configureSession() }
        }
    }

    private func configureSession() {
        guard let device = AVCaptureDevice.default(for: .video),
              let input = try? AVCaptureDeviceInput(device: device),
              session.canAddInput(input) else { return }

        session.addInput(input)

        let output = AVCaptureMetadataOutput()
        guard session.canAddOutput(output) else { return }
        session.addOutput(output)
        output.setMetadataObjectsDelegate(delegate, queue: .main)
        output.metadataObjectTypes = [.qr]

        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            let layer = AVCaptureVideoPreviewLayer(session: self.session)
            layer.videoGravity = .resizeAspectFill
            layer.frame = self.bounds
            self.layer.insertSublayer(layer, at: 0)
            self.previewLayer = layer
        }

        session.startRunning()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        previewLayer?.frame = bounds
    }
}
