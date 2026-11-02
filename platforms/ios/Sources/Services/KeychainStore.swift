import Foundation
import Security

/// Swift-side implementation of the UniFFI SecretStore callback interface,
/// backed by the iOS Keychain (device-only, kSecAttrAccessibleWhenUnlocked,
/// per feature-parity.md section 4).
///
/// TODO(bindings): conform to the generated callback protocol (e.g.
/// `SecretStore`) once A0Platform.swift is generated; the Rust core calls into
/// this object through the UniFFI callback trampoline.
final class KeychainStore {
    static let shared = KeychainStore()
    private let service = "com.jphermans.agentzeromobile"

    private init() {}

    enum KeychainError: Error {
        case unhandled(OSStatus)
    }

    func set(_ secret: String, account: String) throws {
        guard let data = secret.data(using: .utf8) else { return }
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        ]
        let status = SecItemCopyMatching(query as CFDictionary, nil)
        switch status {
        case errSecSuccess:
            let update = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
            guard update == errSecSuccess else { throw KeychainError.unhandled(update) }
        case errSecItemNotFound:
            var add = query
            attributes.forEach { add[$0.key] = $0.value }
            let added = SecItemAdd(add as CFDictionary, nil)
            guard added == errSecSuccess else { throw KeychainError.unhandled(added) }
        default:
            throw KeychainError.unhandled(status)
        }
    }

    func get(account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func delete(account: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
