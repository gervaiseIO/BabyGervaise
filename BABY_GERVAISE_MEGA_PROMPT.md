Before making any changes, bootstrap yourself from the repo first.

You must read and follow the repo’s existing guidance and architecture/product files as binding constraints.

At minimum, inspect and follow:
- AGENTS.md if present
- bootstrap / onboarding / architecture / vision / product law files
- Baby Gervaise PRD / architecture docs
- current prompt config / prompt translator docs if present
- memory-related docs / structs / retrieval logic if present
- Android UI / design guidance if present
- any docs related to runtime, overview, spotify, tools, overlay, and logging/telemetry

Before coding, provide a short preflight:
1. repo files used for bootstrap
2. architecture constraints you will preserve
3. UI constraints you will preserve
4. what you believe is currently broken or unclear
5. how this iteration restores control and observability without drifting into unrelated feature work

Only then proceed.

==================================================
SOURCE OF TRUTH FOR THIS ITERATION
==================================================

Use this as the architectural source of truth for this pass.

Conceptual module ownership:

HGIE
Model
Memory
Tools

Implementation stack:

UI / Overlay
→ ViewModel
→ BabyGervaiseRuntime
→ NativeCoreBridge / JNI
→ Rust Core
   ├─ HGIE
   ├─ Model
   ├─ Memory
   └─ Tools

Preserve these rules:

- HGIE owns interaction/orchestration and conversation behavior
- Model owns inference/runtime
- Memory owns persistence/retrieval
- Tools owns integrations, auth, capability state, and tool actions
- Overview should render structured state from core systems
- HGIE should know which tools are available and integrated
- HGIE should NOT know each tool’s internal implementation details
- Tools is becoming the clean capability facade / internal MCP-like layer for HGIE
- Spotify should become the first clean reference implementation of Tools, but should not distort the architecture into Spotify-specific coupling everywhere

==================================================
THIS ITERATION
==================================================

This is a stabilization + control-plane + tools-facade iteration.

This is NOT:
- a voice iteration
- an Activities iteration
- a broad visual redesign
- a giant feature expansion
- a “finish Spotify fully” pass

This pass should do one deliberate sweep with clear boundaries.

The goals are:

1. Restore Overview as a real control plane
2. Clean up overlay influence and safe boundaries
3. Clean out current Spotify integration and prepare a proper Tools Facade
4. Restore proper debugging / observability / runtime traces
5. Improve Android Logcat instrumentation / telemetry
6. Reduce dead code / refactor debris where safe

==================================================
HARD CONSTRAINTS
==================================================

These are HARD constraints.
Repeat them back in your preflight summary and obey them.

HARD CONSTRAINT 1:
Do not add voice in this iteration.

HARD CONSTRAINT 2:
Do not introduce Activities.

HARD CONSTRAINT 3:
Do not do a broad visual redesign.

HARD CONSTRAINT 4:
Main chat must remain clean and user-facing.
Do not turn main chat into a debug console.

HARD CONSTRAINT 5:
Overview must become the technical control plane again.

HARD CONSTRAINT 6:
Tools must own integrations/auth/capabilities/actions.
HGIE must not absorb tool-specific implementation logic.

HARD CONSTRAINT 7:
Spotify should be reduced into a clean reference tool implementation behind the Tools facade.
Do not keep leaking Spotify logic across the app.

HARD CONSTRAINT 8:
Cloud usage must become clearly verifiable.

HARD CONSTRAINT 9:
We need better runtime debugging and structured observability across UI/runtime/core.

HARD CONSTRAINT 10:
Use Material 3 and Jetpack Compose always.
Use Material 3 and Jetpack Compose always.
Use Material 3 and Jetpack Compose always.

==================================================
PHASE 1 — RESTORE OVERVIEW AS SETTINGS + CONTROL + DIAGNOSTICS
==================================================

Overview is currently too thin and has lost important control surfaces.

Restore Overview as a real settings/control screen with structured sections.

Target direction:

A. Models
- Nano status
- active cloud profile / model config
- model selection restored
- cloud config validity / active profile visibility

B. Memory
- memory status / health summary
- compact useful info only

C. Tools
- tool catalog summary
- available vs integrated tools
- Spotify section from Tools state
- connect/disconnect/auth state

D. Runtime / Diagnostics
- recent turn traces
- routing visibility
- model/tool/memory usage visibility
- recent errors / warnings
- structured diagnostics, not raw chaos by default

E. System
- runtime/app/build state as appropriate

Important:
Overview should feel like a usable Android-native technical cockpit.
Not a giant legacy mess.
Not a toy dashboard.
Not a random dump of raw JSON.

==================================================
PHASE 2 — DEFINE / INSTALL TOOLS FACADE
==================================================

We need a clean facade that HGIE can work through.

Prepare Tools as the clean capability surface for HGIE.

At a conceptual level, Tools should support things like:

- get available tools
- get integrated tools
- get tool catalog
- get tool status
- get tool capabilities
- begin tool auth
- disconnect tool
- refresh tool state
- invoke tool action

HGIE should be able to reason like this:

- Is Spotify available?
- Is Spotify integrated?
- What can Spotify do right now?
- What is the next step required?
- Can I ask Tools to begin auth?
- Can I ask Tools to run an action?

But HGIE should NOT know:
- OAuth internals
- token refresh rules
- endpoint quirks
- vendor-specific capability logic

Make the code move toward this clean separation.

==================================================
PHASE 3 — CLEAN OUT SPOTIFY AND REDUCE IT INTO THE FIRST REFERENCE TOOL
==================================================

Do not try to “finish Spotify.”
Instead, clean it out and re-seat it behind the facade.

Spotify should become the first reference implementation of Tools.

At minimum, Spotify should clearly own:
- auth
- connection state
- capability state
- actions
- presentation mapping into overview-friendly / HGIE-friendly summaries

Under Overview > Tools > Spotify, restore:
- available / not available
- integrated / not integrated
- auth state
- health/basic capability state
- connected account summary if available
- reconnect
- disconnect/remove auth

This should come from Tools, not random UI/runtime assumptions.

If needed, reduce Spotify functionality now in exchange for cleaner boundaries.
Architecture comes first.

==================================================
PHASE 4 — RESTORE ROUTING OBSERVABILITY
==================================================

Right now it is too hard to verify:
- whether cloud is being used
- whether Nano is being used too often
- whether memory was involved
- whether tools were involved
- how HGIE routed a turn

Fix this.

Add a trustworthy diagnostics surface in Overview for recent turns.

For each recent turn, we should be able to inspect something like:
- input source
- memory used or not
- tool consulted or not
- Nano first beat used or not
- cloud escalated or not
- selected cloud profile
- delivery mode
- final route summary
- fallback/error state if relevant

Example conceptual trace:

Turn 128
- input: text
- memory_used: yes
- tool_used: no
- nano_first_beat: yes
- cloud_escalated: yes
- cloud_profile: gemini-2.5-flash-lite
- delivery_mode: NANO_THEN_CLOUD
- final_route: nano + cloud

Naming can adapt to repo style, but the idea must remain.

These traces belong in Overview / Diagnostics, not in the main chat.

==================================================
PHASE 5 — ENHANCE LOGCAT / TELEMETRY / OBSERVABILITY
==================================================

Yes, enhance our Logcat telemetry — but implement it as cleaner structured observability.

Improve:

- Android Logcat instrumentation
- runtime event tracing
- bridge diagnostics
- Rust core tracing where it is surfaced meaningfully
- model/tool/memory/HGIE path observability

Goal:
When debugging, it should be easy to understand from Logcat + Overview:
- what path a turn took
- which module made the decision
- whether tool state was checked
- whether a cloud model was selected
- what failed and where

Prefer structured, tagged logging over noisy ad hoc prints.

Good examples:
- clear tags by subsystem
- route summary lines
- tool auth lifecycle lines
- bridge failure lines
- model selection lines
- cloud escalation decisions
- memory retrieval summary lines

Do not flood the app with meaningless spam logs.

==================================================
PHASE 6 — CLEAN UP OVERLAY INFLUENCE SAFELY
==================================================

The overlay is messy and should not define product direction.

Do not spend this pass trying to make overlay a flagship experience.

Instead:
- identify how overlay is influencing runtime / state / UX today
- reduce bad coupling where safe
- make sure main app behavior does not depend on overlay-specific state
- de-emphasize overlay as an architectural authority

If overlay code remains temporarily, that is acceptable.
But the final direction must be clear:
main chat + Overview are the primary product surfaces.

==================================================
PHASE 7 — DEAD CODE / REFACTOR DEBRIS
==================================================

There is probably dead code and ambiguous code now:
- old Overview components
- dormant settings surfaces
- broken overlay flows
- half-valid Spotify assumptions
- stale tests/assertions
- legacy rendering paths

Do not launch a reckless deletion spree.

Instead:
1. identify dead or ambiguous areas
2. document them in the summary
3. remove only what is clearly safe
4. keep the final active path clear

We want cleaner architecture and safer future iteration, not risky churn.

==================================================
IMPLEMENTATION ORDER
==================================================

Follow this order:

1. inspect repo and confirm current runtime/ui/core state
2. define/install Tools facade direction in code
3. reduce Spotify behind the facade
4. restore Overview sections and controls
5. restore routing diagnostics / cloud verification
6. improve structured Logcat instrumentation / telemetry
7. reduce overlay influence
8. clean safe dead code / refactor debris

Do not jump around randomly.

==================================================
UI / UX GUIDANCE
==================================================

Use Material 3 and Jetpack Compose always.

Overview should feel:
- sectioned
- Android-native
- practical
- expandable when useful
- controlled and readable

Main chat should remain:
- clean
- conversational
- not diagnostic-first

Do not do a broad redesign.
Do only the UI work required to restore a useful control plane.

==================================================
DO NOT LET THESE THINGS HAPPEN
==================================================

Do NOT:
- add voice
- add Activities
- bury model selection again
- bury tool auth state again
- keep Spotify logic leaking into HGIE
- let cloud usage remain unverifiable
- let main chat show raw diagnostics
- reintroduce a giant messy old Overview wholesale
- keep overlay as an architectural center

==================================================
TESTS / VERIFICATION
==================================================

Please add or update tests/checks for:

1. Overview contains restored structured sections
2. Model selection is present under Overview
3. Spotify tool state is present under Overview and sourced from Tools
4. connect/disconnect/auth state is visible for Spotify
5. recent routing traces are visible in Overview
6. routing traces clearly distinguish Nano vs cloud usage
7. cloud usage can be verified through diagnostics
8. structured logging / instrumentation has meaningful subsystem output
9. main chat remains clean and does not show diagnostics/raw debug content
10. overlay is not required for restored control-plane functionality

==================================================
DELIVERABLES
==================================================

Please provide:
1. repo files used for bootstrap
2. files changed
3. summary of what was broken / unclear
4. summary of restored Overview control-plane structure
5. summary of Tools facade introduced or clarified
6. summary of Spotify cleanup / reduction behind the facade
7. summary of routing observability improvements
8. summary of how cloud usage is now verifiable
9. summary of Logcat / telemetry improvements
10. summary of overlay cleanup / de-emphasis
11. summary of safe dead-code cleanup
12. remaining risks / next recommended iteration

==================================================
FINAL REMINDER
==================================================

- restore control before adding features
- Overview becomes Settings + Control + Diagnostics
- Tools becomes HGIE’s clean capability facade
- Spotify becomes the first clean reference tool, not a special architectural exception
- improve Logcat instrumentation / observability
- cloud usage must be verifiable
- main chat stays clean
- no voice in this iteration
- no Activities in this iteration
- Material 3 and Jetpack Compose always
- Material 3 and Jetpack Compose always
- Material 3 and Jetpack Compose always
