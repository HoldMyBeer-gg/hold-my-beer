import Foundation

struct AppConfig: Codable {
    var token: String = ""
    var serverURL: String = "http://localhost:8000"
    var identity: String = "human"
    var setupComplete: Bool = false

    static let key = "collab_config"

    static func load() -> AppConfig {
        guard let data = UserDefaults.standard.data(forKey: key) else { return AppConfig() }
        do {
            return try JSONDecoder().decode(AppConfig.self, from: data)
        } catch {
            print("[AppConfig] Failed to decode saved config, using defaults: \(error)")
            return AppConfig()
        }
    }

    func save() {
        if let data = try? JSONEncoder().encode(self) {
            UserDefaults.standard.set(data, forKey: AppConfig.key)
        }
    }

    var baseURL: URL? { URL(string: serverURL) }

    var authHeader: [String: String] {
        ["Authorization": "Bearer \(token)"]
    }
}
