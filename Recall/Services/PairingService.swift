import Foundation
import Combine

// MARK: - PairingService
// Reads / writes pairing credentials from Keychain.
// QR scanning is handled by QRScannerView → result passed here.

@MainActor
final class PairingService: ObservableObject {
    static let shared = PairingService()

    @Published private(set) var isPaired: Bool = false
    @Published private(set) var pairedDeviceID: String?

    private init() {
        refresh()
    }

    var config: PairingConfig? {
        guard
            let deviceID = KeychainService.read(forKey: AppConstants.pairingDeviceIDKey),
            let secret   = KeychainService.read(forKey: AppConstants.pairingSecretKey),
            let endpoint = KeychainService.read(forKey: AppConstants.pairingEndpointKey)
        else { return nil }
        return PairingConfig(device_id: deviceID, pairing_secret: secret, endpoint: endpoint)
    }

    func pair(with config: PairingConfig) {
        KeychainService.save(config.device_id,      forKey: AppConstants.pairingDeviceIDKey)
        KeychainService.save(config.pairing_secret, forKey: AppConstants.pairingSecretKey)
        KeychainService.save(config.endpoint,       forKey: AppConstants.pairingEndpointKey)
        refresh()
    }

    func unpair() {
        KeychainService.delete(forKey: AppConstants.pairingDeviceIDKey)
        KeychainService.delete(forKey: AppConstants.pairingSecretKey)
        KeychainService.delete(forKey: AppConstants.pairingEndpointKey)
        refresh()
    }

    func parsePairingQR(_ raw: String) throws -> PairingConfig {
        guard let data = raw.data(using: .utf8) else {
            throw PairingError.invalidFormat
        }
        let config = try JSONDecoder().decode(PairingConfig.self, from: data)
        guard !config.device_id.isEmpty, !config.pairing_secret.isEmpty, !config.endpoint.isEmpty else {
            throw PairingError.missingFields
        }
        return config
    }

    private func refresh() {
        let c = config
        isPaired      = c != nil
        pairedDeviceID = c?.device_id
    }
}

enum PairingError: LocalizedError {
    case invalidFormat
    case missingFields

    var errorDescription: String? {
        switch self {
        case .invalidFormat:  return "QR code isn't a valid Recall pairing code."
        case .missingFields:  return "Pairing code is missing required fields."
        }
    }
}
