# Baby Gervaise — Architecture

Version: v0.1  
Status: Prototype Architecture  
Date: 2026  

---

## 1. Purpose

This document defines the **minimal architecture** for Baby Gervaise.

Baby Gervaise is not the full Gervaise runtime.

It is a deliberately constrained Android-first prototype designed to validate:

- one persistent ongoing chat with Gervaise
- the Human–Gervaise Interaction Engine (HGIE)
- lightweight memory and retrieval
- simple tool execution
- one model provider through a single model module
- fast iteration on interaction quality

The architecture must remain intentionally small and easy to evolve.

---

## 2. Architectural Principle

The system must preserve the most important product rule:

**There is one persistent conversation with Gervaise that continues forever.**

There are no:

- chat threads
- session containers
- multiple assistants
- conversation switching
- temporary interaction silos

The user interacts with one continuous Gervaise.

This is the key architectural constraint because the system is being designed around **HGIE continuity**, not chat-product conventions.

---

## 3. Scope of This Architecture

This architecture includes only the primitive runtime necessary for Baby Gervaise.

Included:

- Interface
- HGIE
- Memory
- Tools
- Models
- local persistence
- diagnostics and logs

Explicitly excluded:

- Activity Graph
- Orchestrator
- node-to-node routing
- multi-model orchestration
- background distributed execution
- advanced scheduling
- complex autonomy
- full dynamic UI runtime

Those will belong to later Gervaise phases.

---

## 4. High-Level Architecture

```text
Android App
├── Web UI Shell
├── Android Integration Layer
└── Rust Bridge
    └── Baby Gervaise Core
        ├── HGIE
        ├── Memory
        ├── Tools
        ├── Models
        └── Logging / Metrics
```

Runtime flow:

```text
User Input
→ Interface
→ HGIE
→ Memory Retrieval
→ Model Call
→ Optional Tool Execution
→ Response Assembly
→ Persistent Chat Update
→ UI Render
```

Simplified module chain:

```text
Interface
→ HGIE
→ Memory
→ Tools
→ Models
```

This chain is simple on purpose.

---

## 5. Platform Architecture

### 5.1 Android Host

Baby Gervaise is an **Android application first**.

Responsibilities:

- application lifecycle
- system integration
- Android assistant entry experiments
- hosting the web-based UI shell
- bridging Kotlin ↔ Rust
- local app settings
- future voice input handling

The Android app is the shell and host environment.

---

### 5.2 Web-Based UI Shell

The visual UI is implemented as a lightweight web-based shell embedded inside the Android app.

Responsibilities:

- chat screen rendering
- overview screen rendering
- streaming message updates
- system stats display
- raw input/output log display

The UI must remain minimal and should not contain business logic.

The UI is a surface, not the system.

---

### 5.3 Rust Bridge

A Kotlin ↔ Rust bridge connects the Android host to Baby Gervaise Core.

Responsibilities:

- passing user input into core
- receiving responses from core
- fetching logs and metrics
- reading config values
- exposing tool execution outcomes
- maintaining a stable application boundary

The bridge should remain narrow and predictable.

---

## 6. Baby Gervaise Core

Baby Gervaise Core is the primitive runtime.

It contains five modules:

- HGIE
- Memory
- Tools
- Models
- Logging / Metrics

This core is intentionally small and portable.

---

## 7. Module Definitions

### 7.1 Interface Module

The interface layer is the entry and exit surface for the user.

Responsibilities:

- receive text input
- later receive voice-transcribed input
- display streaming output
- render one persistent conversation
- expose overview and diagnostics

Important rule:

**The interface never creates threads.**

There is only one ongoing chat timeline.

The chat history shown in the interface is a projection of the persistent conversation store.

---

### 7.2 HGIE Module

HGIE is the central runtime module of Baby Gervaise.

Responsibilities:

- interpret the current user message
- inspect recent conversation context
- request additional memory retrieval
- decide whether tools may be useful
- construct the model input
- invoke the model through the Models module
- interpret the result
- trigger deterministic tool execution when appropriate
- assemble the final assistant response
- write updates back to memory and logs

HGIE is the main experimentation surface for this prototype.

The entire architecture exists primarily to support fast iteration on HGIE behavior.

---

### 7.3 Memory Module

Memory is a lightweight local retrieval system.

Responsibilities:

- persist conversation history
- store summaries and extracted facts
- store embeddings / vectors
- perform recent-context retrieval
- perform semantic retrieval
- support configurable “Previous Context” behavior
- provide continuity across app restarts

Storage backend:

- SQLite

Memory should remain simple.

It is not a full memory operating system.
It is a practical retrieval layer for continuity.

---

### 7.4 Tools Module

Tools are capability adapters.

Initial tools:

- Spotify
- Philips Hue

Responsibilities:

- expose deterministic tool interfaces
- validate execution parameters
- call external APIs
- return structured results to HGIE
- log tool calls and outcomes

The LLM does not directly execute tools.

Instead:

1. HGIE decides whether a tool call is appropriate
2. the system validates the tool request
3. the Tools module executes it deterministically
4. HGIE incorporates the result into the reply

This preserves control and explainability.

---

### 7.5 Models Module

The Models module is the single gateway to the LLM provider.

Baby Gervaise uses **one model only**.

Responsibilities:

- load provider configuration
- call the configured external API
- track latency
- track token usage
- record raw prompt and raw output
- return structured model responses to HGIE

The user does not choose models in the UI.

Model selection remains internal.

Configuration is edited before build through a config file.

---

### 7.6 Logging / Metrics Module

The logging layer provides observability for development.

Responsibilities:

- timestamped model logs
- latency tracking
- token usage stats
- tool execution logs
- retrieval logs
- system error logs

These logs are surfaced in the Overview screen.

This is essential for tuning HGIE.

---

## 8. Persistent Conversation Model

Baby Gervaise revolves around one ongoing conversation.

### Core rule

There is one persistent chat timeline.

Every message is appended to the same conversation history.

There is never a “new conversation” concept in Baby Gervaise.

### Implications

- conversation continuity must survive restarts
- retrieval should assume one evolving long-lived interaction
- summaries and semantic memory derive from one unified history
- the UI always renders the same timeline
- HGIE must treat every turn as part of one continuous relationship

This is the key architectural distinction from conventional chat apps.

---

## 9. Data Flow

### 9.1 Standard Conversational Turn

```text
User types message
→ Interface sends message to HGIE
→ HGIE requests recent context from Memory
→ HGIE optionally requests semantic matches from Memory
→ HGIE builds prompt
→ HGIE calls Models module
→ Models module calls configured API
→ Model response returns
→ HGIE optionally decides on tool usage
→ Tools execute if needed
→ HGIE assembles final answer
→ Memory stores turn / summaries / embeddings
→ Logging stores diagnostics
→ Interface renders response
```

---

### 9.2 Tool-Enabled Turn

```text
User asks to control Hue or Spotify
→ HGIE interprets intent
→ HGIE retrieves context if needed
→ HGIE prepares structured tool request
→ Tools module validates and executes request
→ Result returns to HGIE
→ HGIE produces natural response
→ Response stored in persistent conversation
```

---

### 9.3 Overview Screen Flow

```text
Overview opened
→ Interface requests logs and metrics
→ Logging / Metrics module returns stats
→ Memory returns retrieval counts / vector counts
→ Models returns token / latency stats
→ UI renders overview dashboard
```

---

## 10. Memory Retrieval Strategy

Baby Gervaise uses a simple retrieval policy controlled by the **Previous Context** setting.

### Low

- retrieve only very recent conversation
- minimal semantic retrieval
- smallest context footprint

### Medium

- retrieve recent conversation
- retrieve some semantically relevant memory
- balanced continuity and performance

### High

- retrieve more conversation history
- retrieve deeper semantic matches
- maximize continuity and recall

The purpose of this setting is experimentation, not user complexity.

It helps validate how much context HGIE actually needs.

---

## 11. Configuration Architecture

Configuration is build-time editable.

Suggested config areas:

```text
/config/model_config.json
/config/prompt_config.json
/config/app_config.json
```

### Model config

Defines:

- provider
- endpoint
- API key
- model name
- temperature
- timeout

### Prompt config

Defines:

- system prompt
- behavior instructions
- tool usage guidance
- memory injection format

### App config

Defines:

- default Previous Context level
- logging verbosity
- diagnostics toggles

Prompt configuration should be easy to edit before build to support fast HGIE iteration.

---

## 12. Persistence Architecture

Persist locally:

- message history
- memory summaries
- vector entries
- key facts
- model logs
- token and latency stats
- config snapshots if needed

Persistence goals:

- restart-safe continuity
- inspectability
- lightweight local-first behavior

---

## 13. UI Architecture

### 13.1 Chat Screen

The main screen shows:

- one continuous timeline
- user messages
- Gervaise responses
- streaming in-progress state
- message input

There is no thread list.

There is no “start new chat”.

There is one chat with Gervaise.

---

### 13.2 Overview Screen

The second screen shows:

- model statistics
- memory statistics
- system statistics
- raw prompt / raw output logs
- timestamps
- simple configuration controls for Previous Context

This screen exists primarily for development and validation.

---

## 14. Android Assistant Path

Baby Gervaise should be shaped so it can later experiment with becoming an Android assistant entry point.

For this phase, the architecture should keep room for:

- assistant launch intent entry
- text-first assistant interaction
- later voice-first assistant interaction

This should not complicate the core runtime.

The assistant entry point should still feed into the same HGIE pipeline.

One assistant entry.
One HGIE.
One persistent chat.

---

## 15. Voice Evolution Path

Voice is not the first implementation focus, but the architecture should support it later.

Future flow:

```text
Voice Input
→ Speech-to-Text
→ HGIE
→ Memory / Tools / Models
→ Response
→ Optional Text-to-Speech
```

Voice should become another input mode into the same persistent conversation.

It must not create a separate assistant runtime.

---

## 16. Proposed Project Structure

```text
baby-gervaise/
├── android/
│   ├── app/
│   └── bridge/
├── ui_web/
│   ├── src/
│   └── assets/
├── rust_core/
│   ├── src/
│   │   ├── hgie.rs
│   │   ├── memory.rs
│   │   ├── model.rs
│   │   ├── tools.rs
│   │   ├── logging.rs
│   │   └── lib.rs
│   └── Cargo.toml
├── config/
│   ├── model_config.json
│   ├── prompt_config.json
│   └── app_config.json
└── docs/
    ├── BABY_GERVAISE_PRD.md
    └── BABY_GERVAISE_ARCHITECTURE.md
```

---

## 17. Architectural Success Criteria

The architecture is successful if it enables:

- one persistent forever-chat with Gervaise
- fast iteration on HGIE prompts and behavior
- simple memory retrieval and persistence
- deterministic tool execution
- one configurable model provider through one module
- clear diagnostics and observability
- a clean Android-first prototype path

The architecture is not trying to prove full Gervaise.

It is trying to prove the smallest meaningful living version of Gervaise.

---

## 18. One Sentence Summary

Baby Gervaise architecture is a minimal Android-first runtime built around one persistent ongoing chat, a central HGIE, lightweight local memory, deterministic tools, and a single configurable model module.
