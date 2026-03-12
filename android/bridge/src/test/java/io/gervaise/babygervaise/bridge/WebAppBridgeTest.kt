package io.gervaise.babygervaise.bridge

import org.junit.Assert.assertEquals
import org.junit.Test

class WebAppBridgeTest {
    @Test
    fun parseCommandReadsCommandAndPayload() {
        val bridge = WebAppBridge(
            nativeCore = NativeBabyGervaise(),
            emitToWeb = { _, _ -> },
        )

        val command = bridge.parseCommand(
            """{"command":"set_context_level","payload":{"level":"high"}}"""
        )

        assertEquals("set_context_level", command.type)
        assertEquals("high", command.payload.getString("level"))
    }
}
