package io.gervaise.babygervaise

internal object InteractionTimelineDemoData {
    val timeline: List<InteractionItem> = listOf(
        UserBubble(
            id = "user-demo-1",
            timestampMs = 1_000L,
            turnId = "demo-turn-1",
            text = "Play the Beatles on Denon",
        ),
        AssistantBubble(
            id = "assistant-demo-1",
            timestampMs = 2_000L,
            turnId = "demo-turn-1",
            text = "Sure. I'm checking Spotify and your playback devices.",
            isStreaming = false,
        ),
        ActionCard(
            id = "action-demo-1",
            timestampMs = 3_000L,
            turnId = "demo-turn-1",
            tool = "Spotify",
            title = "Checking connection",
            status = "Running",
            detail = "Reviewing account state and available devices.",
        ),
        ProgressCard(
            id = "progress-demo-1",
            timestampMs = 4_000L,
            turnId = "demo-turn-1",
            tool = "Spotify",
            title = "Device discovery",
            detail = "Checking available devices...",
        ),
        ActionResultCard(
            id = "result-demo-1",
            timestampMs = 5_000L,
            turnId = "demo-turn-1",
            tool = "Spotify",
            title = "Spotify connected",
            detail = "Playback is ready on Denon.",
            status = "Success",
            supportingLines = listOf("Device: Denon"),
            tone = InteractionTone.Positive,
        ),
        LiveStateCard(
            id = "live-demo-1",
            timestampMs = 6_000L,
            title = "Now playing",
            detail = "The Beatles - Come Together",
            supportingLines = listOf("Device: Denon"),
            tone = InteractionTone.Neutral,
        ),
        SuggestionCard(
            id = "suggestion-demo-1",
            timestampMs = 7_000L,
            title = "Which device should I use?",
            options = listOf(
                SuggestionOption(id = "phone", label = "Phone"),
                SuggestionOption(id = "denon", label = "Denon"),
                SuggestionOption(id = "tv", label = "TV"),
            ),
        ),
        ErrorCard(
            id = "error-demo-1",
            timestampMs = 8_000L,
            turnId = "demo-turn-2",
            title = "Spotify couldn't start playback",
            detail = "Premium may be required.",
            supportingLines = listOf("Reconnect Spotify or try another device."),
        ),
    )
}
