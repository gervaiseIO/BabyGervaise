# Baby Gervaise --- Product Requirements Document (PRD)

Version: v0.2\
Status: Prototype Definition\
Date: 2026

------------------------------------------------------------------------

# 1. Purpose

Baby Gervaise is the **first minimal embodiment of the Gervaise
system**.

It is an intentionally constrained Android prototype designed to
validate the most important concept of the Gervaise vision:

**A human interacts with one continuous intelligence.**

Baby Gervaise focuses on rapidly iterating the **Human--Gervaise
Interaction Engine (HGIE)** while keeping the system simple,
understandable, and fast to evolve.

This prototype deliberately avoids large architectural complexity in
order to focus on:

-   interaction quality
-   conversational continuity
-   lightweight memory
-   early capability integrations
-   experimentation speed

Baby Gervaise is **not the full Gervaise platform**.

It is a small, focused environment that helps validate the core
interaction model before introducing the full system architecture.

------------------------------------------------------------------------

# 2. Foundational Principle --- One Continuous Chat

The **most important rule of Baby Gervaise**:

There is **one persistent conversation with Gervaise that never
resets.**

There are:

-   no chat threads\
-   no conversations\
-   no assistants\
-   no sessions\
-   no message containers

There is **one continuous chat with Gervaise that persists forever.**

The user never chooses a conversation.\
The user simply continues interacting with Gervaise.

Example mental model:

> I am speaking with Gervaise, and the conversation continues.

This rule is critical because it allows HGIE to evolve into a true
**continuous interaction engine**, rather than a chat interface.

------------------------------------------------------------------------

# 3. Alignment With Gervaise Product Laws

Baby Gervaise must respect the foundational doctrine of the Gervaise
system.

Key laws include:

-   one continuous interface
-   capabilities instead of apps
-   model invisibility
-   internal system complexity hidden from users
-   deterministic execution

These principles ensure Baby Gervaise remains aligned with the long‑term
system identity.

------------------------------------------------------------------------

# 4. Product Definition

Baby Gervaise is an **Android assistant application** that provides:

-   one persistent chat with Gervaise
-   conversational interaction with an AI model
-   simple memory retrieval
-   lightweight semantic continuity
-   capability integrations

The prototype is intentionally minimal.

It exists primarily to refine **HGIE behavior and interaction design**.

------------------------------------------------------------------------

# 5. Product Goals

Baby Gervaise aims to validate the following core concepts.

## 5.1 HGIE Interaction Quality

HGIE must be able to:

-   interpret natural user input
-   maintain conversational continuity
-   incorporate memory context
-   trigger tool actions
-   respond coherently

HGIE is the primary focus of the prototype.

------------------------------------------------------------------------

## 5.2 Continuous Conversation

The system must demonstrate a **single continuous conversation** that
persists indefinitely.

The conversation should:

-   retain context
-   use memory retrieval
-   support long‑term interaction

------------------------------------------------------------------------

## 5.3 Lightweight Memory

Baby Gervaise must demonstrate basic memory capabilities:

-   storing information from conversations
-   retrieving relevant context
-   improving conversation continuity

Memory is intentionally simple in this phase.

------------------------------------------------------------------------

## 5.4 Capability Integrations

The system should demonstrate the **capabilities-not-apps** principle.

Initial integrations:

-   Spotify
-   Philips Hue

These capabilities allow Gervaise to perform useful actions during
conversation.

------------------------------------------------------------------------

## 5.5 Android Assistant Experiments

Baby Gervaise should allow experimentation with:

-   assistant-style voice interaction
-   potentially acting as the Android assistant entry point

------------------------------------------------------------------------

## 5.6 Fast Iteration

The architecture must remain simple so HGIE behavior can be refined
quickly.

------------------------------------------------------------------------

# 6. Non-Goals

The following systems are intentionally excluded from Baby Gervaise.

-   Activity Graph
-   Orchestrator
-   distributed nodes
-   model routing
-   local model runtime
-   multi-device synchronization
-   advanced scheduling
-   developer extension systems

These systems may appear in later phases.

------------------------------------------------------------------------

# 7. Platform

Primary platform:

Android

Technology stack:

-   Kotlin Android application
-   Web-based UI shell
-   Rust core library (Baby Gervaise Core)

Rust provides a portable core foundation for future platforms.

------------------------------------------------------------------------

# 8. System Architecture (Simplified)

Baby Gervaise uses a minimal architecture.

Interface\
→ HGIE\
→ Memory\
→ Tools\
→ Models

This structure allows quick iteration while preserving future
architectural expansion.

------------------------------------------------------------------------

# 9. Interface

The UI must remain extremely simple.

Two screens exist in Baby Gervaise.

------------------------------------------------------------------------

## 9.1 Chat Screen

The primary interface.

Features:

-   message input
-   streaming responses
-   conversation history
-   assistant-style interaction

The conversation is **persistent and continuous**.

There is **only one chat with Gervaise**.

------------------------------------------------------------------------

## 9.2 Overview Screen

Accessible from the top-right button.

This screen acts as a **system inspection dashboard**.

### Model Statistics

-   model name
-   total calls
-   token usage
-   latency metrics

### Memory Statistics

-   number of stored memories
-   vector count
-   retrieval usage

### System Statistics

-   interactions
-   tool calls
-   system events

------------------------------------------------------------------------

## 9.3 Raw Model Logs

The Overview screen includes a **raw log viewer** showing:

-   prompt input
-   model output
-   timestamps

This allows direct inspection of HGIE behavior.

------------------------------------------------------------------------

## 9.4 Configuration

A simple configuration option exists for **Previous Context Retrieval**.

Settings:

Low\
Medium\
High

These control how much past context is retrieved when generating
responses.

Low

-   minimal memory retrieval
-   small context window

Medium

-   recent conversation
-   moderate retrieval

High

-   deeper semantic retrieval
-   more historical context

This allows experimentation with memory behavior.

------------------------------------------------------------------------

# 10. HGIE (Human--Gervaise Interaction Engine)

HGIE is the central intelligence layer.

Responsibilities:

-   interpret user input
-   retrieve memory context
-   construct prompts
-   call the AI model
-   process responses
-   optionally trigger tools

HGIE should behave like a **personal assistant interpreter**, not a
chatbot.

------------------------------------------------------------------------

# 11. Memory

Memory provides continuity for the persistent conversation.

Implementation:

-   SQLite storage
-   vector embeddings

Vectors allow simple semantic retrieval.

Memory types may include:

-   conversation summaries
-   key user facts
-   contextual interaction notes

------------------------------------------------------------------------

# 12. Models

Baby Gervaise uses **one conversational AI model**.

Characteristics:

-   strong conversational ability
-   stable responses
-   moderate reasoning ability

Model choice is **internal to the system**.

Users never select models.

------------------------------------------------------------------------

## Model Module Responsibilities

The model module manages:

-   API calls
-   token tracking
-   latency metrics
-   request logging

These metrics feed the Overview dashboard.

------------------------------------------------------------------------

# 13. Tools (Capabilities)

Initial capabilities:

## Spotify

Possible actions:

-   play music
-   pause playback
-   search songs
-   control volume

------------------------------------------------------------------------

## Philips Hue

Possible actions:

-   turn lights on/off
-   adjust brightness
-   change colors
-   activate scenes

These demonstrate capability integration.

------------------------------------------------------------------------

# 14. Voice (Future Iteration)

Voice will be added after text interaction stabilizes.

Future voice features:

-   microphone input
-   speech-to-text
-   conversational response
-   optional speech output

------------------------------------------------------------------------

# 15. Persistence

The system must persist across restarts.

Stored data:

-   conversation history
-   memory vectors
-   system metrics
-   configuration settings

------------------------------------------------------------------------

# 16. Success Criteria

Baby Gervaise succeeds if it demonstrates:

-   continuous assistant-like interaction
-   working memory retrieval
-   reliable conversational responses
-   successful Spotify and Hue integrations
-   clear HGIE behavior visibility

The system should feel like a **small but real personal assistant**.

------------------------------------------------------------------------

# 17. One Sentence Definition

Baby Gervaise is a **minimal Android implementation of Gervaise designed
to iterate rapidly on a continuous assistant interaction powered by
HGIE, lightweight memory, and capability integrations.**
