package io.gervaise.babygervaise.bridge

fun interface NanoPromptRunner {
    fun runNanoPrompt(requestJson: String): String
}

interface NanoHost : NanoPromptRunner {
    fun loadNanoSnapshot(): String
}
