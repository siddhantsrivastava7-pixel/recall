import Foundation

struct PairingConfig: Codable {
    let device_id: String
    let pairing_secret: String
    let endpoint: String
}
