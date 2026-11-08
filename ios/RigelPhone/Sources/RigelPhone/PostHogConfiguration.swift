import Foundation
import PostHog

enum PostHogConfiguration {
    static func setup() {
        guard let projectToken = value(for: "POSTHOG_PROJECT_TOKEN") else {
            reportMissingConfiguration("POSTHOG_PROJECT_TOKEN")
            return
        }

        guard let host = value(for: "POSTHOG_HOST") else {
            reportMissingConfiguration("POSTHOG_HOST")
            return
        }

        let config = PostHogConfig(projectToken: projectToken, host: host)
        config.errorTrackingConfig.autoCapture = true
        config.logs.serviceName = "rigel-phone"
        PostHogSDK.shared.setup(config)
    }

    private static func value(for key: String) -> String? {
        let environmentValue = ProcessInfo.processInfo.environment[key]
        let bundledValue = Bundle.main.object(forInfoDictionaryKey: key) as? String
        let value = environmentValue ?? bundledValue
        return value?.isEmpty == false ? value : nil
    }

    private static func reportMissingConfiguration(_ key: String) {
        #if DEBUG
        assertionFailure("\(key) variable required by PostHog is missing or un-configured, this causes events to be silently missed. This error stops appearing once \(key) is configured")
        #endif
    }
}
