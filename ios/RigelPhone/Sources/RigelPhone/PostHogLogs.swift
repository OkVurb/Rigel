import PostHog

/// The only app log lines exported to PostHog. Existing app logging remains local.
enum PostHogLogs {
    static func roomConnectionEstablished(agentCount: Int, channelCount: Int) {
        PostHogSDK.shared.logger?.info(
            "room connection established",
            attributes: [
                "agent_count": agentCount,
                "channel_count": channelCount,
            ]
        )
    }

    static func roomConnectionFailed() {
        PostHogSDK.shared.logger?.warn("room connection failed")
    }

    static func cloudSignInSucceeded() {
        PostHogSDK.shared.logger?.info("cloud sign-in succeeded")
    }
}
