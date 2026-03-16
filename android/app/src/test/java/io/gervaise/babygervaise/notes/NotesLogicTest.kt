package io.gervaise.babygervaise.notes

import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotesLogicTest {
    @Test
    fun provisionalTitleUsesMeaningfulBodyText() {
        assertEquals(
            "HGIE prompt routing cleanup for tomorrow",
            deriveSystemProvisionalTitle("HGIE prompt routing cleanup for tomorrow"),
        )
        assertEquals(
            UNTITLED_NOTE_TITLE,
            deriveSystemProvisionalTitle("   \n\n  "),
        )
    }

    @Test
    fun materializeGateRejectsWhitespaceOnlyBodies() {
        assertFalse(" \n\t ".isMeaningfulNoteBody())
        assertTrue("Real content".isMeaningfulNoteBody())
    }

    @Test
    fun lexicalSearchUsesTitlePathAndExcerpt() {
        val items = listOf(
            NoteListItem(
                noteRef = NoteRef("1", "Architecture/HGIE.md"),
                title = "HGIE Prompt Rephase",
                excerpt = "Refined routing and runtime boundaries",
                titleLifecycle = NoteTitleLifecycle.SYSTEM_STABLE,
                lastSavedAt = "2026-03-15T12:00:00Z",
            ),
            NoteListItem(
                noteRef = NoteRef("2", "Journal/Daily.md"),
                title = "Daily note",
                excerpt = "Walked through search and recents",
                titleLifecycle = NoteTitleLifecycle.USER_OR_IMPORTED,
                lastSavedAt = "2026-03-14T12:00:00Z",
            ),
        )

        assertEquals(1, lexicalSearchCatalog(items, "runtime").size)
        assertEquals(1, lexicalSearchCatalog(items, "journal").size)
        assertEquals(2, lexicalSearchCatalog(items, "").size)
    }

    @Test
    fun continuityWindowIsBounded() {
        val now = Instant.parse("2026-03-15T12:00:00Z")

        assertTrue(shouldResumeLastActiveNote("2026-03-15T05:00:00Z", now))
        assertFalse(shouldResumeLastActiveNote("2026-03-14T00:00:00Z", now))
    }
}
