package io.gervaise.babygervaise

import org.junit.Assert.assertEquals
import org.junit.Test

class MlKitNanoHostTest {
    @Test
    fun normalizeNanoMaxOutputTokensUsesModeFallbackWhenMissing() {
        assertEquals(48, normalizeNanoMaxOutputTokens(null, NanoPromptModeWire.FIRST_BEAT))
        assertEquals(40, normalizeNanoMaxOutputTokens(null, NanoPromptModeWire.AMBIENT))
        assertEquals(192, normalizeNanoMaxOutputTokens(null, NanoPromptModeWire.FULL_REPLY))
    }

    @Test
    fun normalizeNanoMaxOutputTokensClampsValuesAboveMlKitLimit() {
        assertEquals(256, normalizeNanoMaxOutputTokens(512, NanoPromptModeWire.FULL_REPLY))
    }

    @Test
    fun normalizeNanoMaxOutputTokensFallsBackWhenValueIsBelowRange() {
        assertEquals(48, normalizeNanoMaxOutputTokens(0, NanoPromptModeWire.FIRST_BEAT))
        assertEquals(192, normalizeNanoMaxOutputTokens(-4, NanoPromptModeWire.FULL_REPLY))
    }
}
