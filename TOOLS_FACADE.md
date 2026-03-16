# Tools Facade — Baby Gervaise

## Purpose

This document defines the **Tools Facade** for Baby Gervaise.

Its job is to keep tool integrations clean, generic, and future-friendly so that:

- **HGIE** can reason about tools without knowing their internal implementation
- **Overview** can render tool state from a stable source of truth
- each integration can own its own auth, state, and actions
- Baby Gervaise can gradually evolve into a richer capability system without polluting HGIE

This is the reference architecture for how **Tools** should behave in Baby Gervaise.

---

# 1. Core Architecture Context

Baby Gervaise has four core conceptual modules:

```text
HGIE
Model
Memory
Tools
```

At runtime, the implementation stack is roughly:

```text
UI / Overlay
→ ViewModel
→ BabyGervaiseRuntime
→ NativeCoreBridge / JNI
→ Rust Core
   ├─ HGIE
   ├─ Model
   ├─ Memory
   └─ Tools
```

Within that architecture:

- **HGIE** owns interaction and orchestration
- **Model** owns inference/runtime
- **Memory** owns persistence/retrieval
- **Tools** owns integrations, auth, capabilities, and actions

---

# 2. Why Tools Facade Exists

Without a facade, HGIE slowly becomes polluted with integration-specific logic such as:

- Spotify auth details
- endpoint quirks
- token refresh logic
- device playback conditions
- vendor-specific error handling

That is not scalable.

The Tools Facade exists so HGIE can ask simple questions like:

- what tools exist?
- which tools are available?
- which tools are integrated?
- what can this tool do right now?
- what is the next step needed?
- can this action be executed now?

and then convert those answers into a user-facing conversational response.

The main rule is:

## HGIE speaks. Tools knows how the integration works.

---

# 3. Ownership Rules

## HGIE owns

- conversational interaction
- user intent handling
- deciding whether a tool is needed
- deciding whether memory/model/tool should be used
- deciding how to phrase the next step to the user
- deciding whether a tool result should be summarized in chat

## Tools owns

- integration discovery
- auth state
- token lifecycle
- integration-specific configuration
- capability state
- action execution
- connection health
- mapping raw tool state into structured tool summaries

## Overview owns

- rendering tool state from Tools
- exposing tool controls such as connect/disconnect
- showing tool health/basic metadata

Overview does **not** become the tool source of truth.

## Memory owns

- storing conversational/user/system facts
- retrieving relevant memory when HGIE asks for it

Memory is not the tool registry.

## Model owns

- model inference/runtime only

The model does not own auth or integration implementation details.

---

# 4. The Core Tools Facade Contract

The Tools module should expose a **generic contract** to HGIE.

Conceptually, HGIE should not need to know Spotify internals, Hue internals, or future tool internals.

The facade should support five major areas:

## 4.1 Tool discovery

The system should be able to answer:

- what tools are known?
- which tools are available on this build/device?
- which tools are currently integrated/configured?

Conceptual contract:

```text
get_available_tools()
get_integrated_tools()
get_tool_catalog()
```

## 4.2 Tool status

For each tool, HGIE and Overview should be able to retrieve a structured status object.

Conceptual contract:

```text
get_tool_status(tool_id)
```

The returned status should be generic and structured.

## 4.3 Tool capabilities

HGIE should be able to ask what a tool can do right now.

Conceptual contract:

```text
get_tool_capabilities(tool_id)
```

This should reflect runtime reality, not wishful static capability only.

Example:
Spotify may support playback in general, but if no active device exists then capability may be degraded.

## 4.4 Tool lifecycle / auth actions

HGIE and Overview should be able to trigger lifecycle operations generically.

Conceptual contract:

```text
begin_tool_auth(tool_id)
disconnect_tool(tool_id)
refresh_tool_state(tool_id)
```

## 4.5 Tool action execution

HGIE should be able to invoke tool actions generically.

Conceptual contract:

```text
invoke_tool_action(tool_id, action_id, params)
```

HGIE should not know the vendor-specific transport details behind that action.

---

# 5. Tool Status Model

A tool should expose a stable high-level status model.

Conceptually, each tool should provide:

## Identity

- `tool_id`
- `display_name`
- `category`

## Availability

- `available`: whether the tool exists in this build/runtime
- `integrated`: whether the tool has been configured/authenticated enough to be used

## Auth / connection state

Suggested high-level states:

- `not_required`
- `required_not_started`
- `auth_in_progress`
- `connected`
- `expired`
- `error`

## Health state

Suggested high-level states:

- `healthy`
- `degraded`
- `unavailable`
- `error`

## Next step guidance

This is important.

Each tool should be able to tell HGIE what the next action is.

Suggested examples:

- `ready`
- `auth_required`
- `reconnect_required`
- `permission_required`
- `missing_target_device`
- `configuration_required`
- `temporary_error`

HGIE can then convert that into user-facing conversation.

---

# 6. Example HGIE Interaction with Tools

## Example 1 — Connect Spotify

User:
> Let’s login to Spotify

HGIE:
1. asks Tools for Spotify status
2. Tools returns:
   - available = true
   - integrated = false
   - next_step = auth_required
3. HGIE replies:
   - “Sure — Spotify is available, but first we need to connect it. Want to do that now?”

The auth flow remains owned by Tools.

## Example 2 — Use Spotify before auth

User:
> Play jazz on Spotify

HGIE:
1. asks Tools about Spotify
2. Tools says:
   - available = true
   - integrated = false
   - next_step = auth_required
3. HGIE replies:
   - “I can do that, but Spotify isn’t linked yet. Let’s connect it first.”

## Example 3 — Connected but limited capability

User:
> Play my playlist

HGIE:
1. asks Tools about Spotify capability
2. Tools says:
   - connected = true
   - health = degraded
   - next_step = missing_target_device
3. HGIE replies:
   - “Spotify is connected, but there isn’t an active playback device right now.”

Again: HGIE speaks, Tools informs.

---

# 7. Spotify as the First Reference Tool

Spotify should be treated as the **first reference implementation** of the Tools Facade.

That means Spotify should help shape the facade, but should not distort it into something Spotify-specific only.

## Spotify should eventually own

- auth flow
- callback handling
- token lifecycle
- persistent connection state
- capability inspection
- action execution
- mapping raw API state into clean tool summaries

## Spotify should not own

- HGIE conversation behavior
- Overview rendering policy
- main chat formatting rules
- generic tool registry behavior

---

# 8. Suggested Internal Spotify Structure

Inside Tools, Spotify can still be broken into sub-parts.

Conceptual structure:

```text
Tools
└── Spotify
    ├── Auth
    ├── State
    ├── Capabilities
    ├── Actions
    └── Presentation
```

## Auth
Handles:
- login
- callback consumption
- refresh/reconnect
- disconnect

## State
Handles:
- persisted connection state
- account identity summary
- last known relevant metadata

## Capabilities
Handles:
- what Spotify can do right now
- whether playback is possible
- whether a device is available
- whether auth is valid

## Actions
Handles:
- play
- pause
- next
- select target
- query account/device state

## Presentation
Handles:
- mapping raw Spotify/internal state into simple structured summaries
- producing overview-friendly metadata
- producing HGIE-friendly next-step guidance

---

# 9. Overview Integration

Overview should render tool state **from Tools**.

It should not reconstruct state ad hoc from scattered runtime/UI logic.

## Overview > Tools should show

For each tool, ideally:

- tool name
- available or not
- integrated or not
- auth state
- health state
- next step
- basic summary metadata

## Spotify section should show at minimum

- connected / not connected
- account display info if available
- auth validity / state
- reconnect
- disconnect/remove connection
- basic capability or health summary

This is enough for the current stage.

---

# 10. Relationship to Debugging / Diagnostics

The Tools Facade should also support observability.

That means the system should be able to inspect:

- which tools are available
- which tools are integrated
- what last-known tool state is
- what action failed and why
- recent tool errors/warnings if appropriate

This does not mean dumping raw internal blobs into the main chat.

Diagnostics belong in Overview / telemetry surfaces.

---

# 11. Relationship to Logcat / Telemetry

The wording **“enhance our Logcat telemetry”** is understandable, but a cleaner way to describe the work is:

- **improve Android Logcat instrumentation**
- **improve runtime telemetry and diagnostics**
- **improve structured logging across UI/runtime/core**
- **improve observability for model/tool/memory routing**

“Telemetry” is fine if you mean internal event traces and diagnostics.
If you mean actual Logcat output specifically, “structured Logcat instrumentation” is even clearer.

Recommended wording in engineering tasks:

## Preferred phrasing
- Improve runtime observability
- Add structured Logcat instrumentation
- Add per-turn routing telemetry
- Improve diagnostics across UI/runtime/core
- Add tool/model/memory traces for debugging

---

# 12. Non-Goals Right Now

The Tools Facade does **not** mean:

- building every future tool immediately
- overengineering a giant plugin framework
- introducing Activities now
- moving all product behavior into Tools
- making HGIE passive

HGIE still remains the interaction brain.

Tools is the capability surface HGIE works through.

---

# 13. Short Source-of-Truth Summary

## Core rule

**HGIE should know what tools are available, integrated, and capable of; Tools should know how those integrations actually work.**

## Architectural summary

- HGIE orchestrates
- Tools integrates
- Overview renders tool state from Tools
- Spotify is the first reference implementation
- the facade should stay generic so Baby Gervaise can grow cleanly

---

# 14. Future Evolution

This facade is the right stepping stone toward a richer system later.

Today:
- it is a clean integration boundary for Baby Gervaise

Later:
- it can evolve into a broader internal capability system
- possibly closer to a true internal MCP-like tool layer for Gervaise

But right now, the priority is:
- clarity
- cleanliness
- observability
- not polluting HGIE
