package io.gervaise.babygervaise.notes

import java.security.MessageDigest
import java.text.Normalizer
import java.time.Duration
import java.time.Instant
import java.util.Locale

private val markdownPrefixRegex = Regex("""^[#>\-\*\d\.\)\s]+""")
private val whitespaceRegex = Regex("""\s+""")
private val slugInvalidRegex = Regex("""[^a-z0-9]+""")

fun String.isMeaningfulNoteBody(): Boolean = trim().isNotEmpty()

fun deriveSystemProvisionalTitle(body: String): String {
    val candidate = body.lineSequence()
        .map(String::trim)
        .firstOrNull(String::isNotBlank)
        ?.replace(markdownPrefixRegex, "")
        ?.replace(whitespaceRegex, " ")
        ?.trim()
        .orEmpty()
    if (candidate.isBlank()) {
        return UNTITLED_NOTE_TITLE
    }
    return candidate
        .split(' ')
        .take(7)
        .joinToString(" ")
        .take(48)
        .trim()
        .ifBlank { UNTITLED_NOTE_TITLE }
}

fun createNoteExcerpt(
    body: String,
    maxChars: Int = 160,
): String {
    val normalized = body
        .replace("\r\n", "\n")
        .lineSequence()
        .map(String::trim)
        .filter(String::isNotBlank)
        .joinToString(" ")
        .replace(whitespaceRegex, " ")
        .trim()
    if (normalized.length <= maxChars) {
        return normalized
    }
    val truncatedLength = if (maxChars <= 1) 0 else maxChars - 1
    return normalized.take(truncatedLength).trimEnd() + "…"
}

fun slugifyTitle(title: String): String {
    val ascii = Normalizer.normalize(title.lowercase(Locale.US), Normalizer.Form.NFD)
        .replace(Regex("""\p{Mn}+"""), "")
    val slug = ascii
        .replace(slugInvalidRegex, "-")
        .trim('-')
    return slug.ifBlank { "untitled-note" }
}

fun buildStoredMarkdown(
    title: String,
    body: String,
    mirrorsTitleToH1: Boolean,
): String {
    val normalizedBody = body.replace("\r\n", "\n")
    return if (mirrorsTitleToH1) {
        "# ${title.ifBlank { UNTITLED_NOTE_TITLE }}\n\n$normalizedBody"
    } else {
        normalizedBody
    }
}

fun parseStoredMarkdown(
    content: String,
    fallbackTitle: String,
): ParsedMarkdownNote {
    val normalized = content.replace("\r\n", "\n")
    val lines = normalized.lines()
    val firstMeaningfulIndex = lines.indexOfFirst { it.trim().isNotEmpty() }
    if (firstMeaningfulIndex >= 0) {
        val firstMeaningfulLine = lines[firstMeaningfulIndex].trim()
        if (firstMeaningfulLine.startsWith("# ")) {
            val title = firstMeaningfulLine.removePrefix("# ").trim().ifBlank { fallbackTitle }
            val remaining = lines.drop(firstMeaningfulIndex + 1)
                .joinToString("\n")
                .removePrefix("\n")
                .removePrefix("\n")
            return ParsedMarkdownNote(
                title = title,
                body = remaining,
                mirrorsTitleToH1 = true,
            )
        }
    }
    return ParsedMarkdownNote(
        title = fallbackTitle.ifBlank { UNTITLED_NOTE_TITLE },
        body = normalized,
        mirrorsTitleToH1 = false,
    )
}

fun noteKeyForRelativePath(relativePath: String): String = sha256Hex(relativePath.trim())

fun contentSha256(markdown: String): String = sha256Hex(markdown)

fun shouldResumeLastActiveNote(
    lastOpenedAt: String?,
    now: Instant,
    continuityWindow: Duration = Duration.ofHours(12),
): Boolean {
    val timestamp = runCatching { Instant.parse(lastOpenedAt) }.getOrNull() ?: return false
    return !timestamp.isBefore(now.minus(continuityWindow))
}

fun lexicalSearchCatalog(
    items: List<NoteListItem>,
    query: String,
): List<NoteListItem> {
    val normalizedQuery = query.trim().lowercase(Locale.US)
    if (normalizedQuery.isBlank()) {
        return items.sortedByDescending { it.rankTimestamp.orEmpty() }
    }
    return items
        .filter { item ->
            item.title.lowercase(Locale.US).contains(normalizedQuery) ||
                item.noteRef.relativePath.lowercase(Locale.US).contains(normalizedQuery) ||
                item.excerpt.lowercase(Locale.US).contains(normalizedQuery)
        }
        .sortedByDescending { it.rankTimestamp.orEmpty() }
}

private fun sha256Hex(value: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(value.toByteArray())
    return buildString(digest.size * 2) {
        digest.forEach { byte ->
            append("%02x".format(byte))
        }
    }
}
