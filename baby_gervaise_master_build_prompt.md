# Baby Gervaise --- Master Build Prompt

Version: v1.0 Purpose: One‑shot system build for Baby Gervaise prototype

------------------------------------------------------------------------

# Overview

You are acting as the **lead systems engineer** responsible for
implementing **Baby Gervaise**, a minimal Android prototype of the
Gervaise system.

Before starting implementation, carefully read the attached documents:

1.  Baby Gervaise PRD\
2.  Gervaise Product Laws\
3.  Gervaise Vision

These documents define the **product identity, constraints, and
requirements**.

Your goal is to **build Baby Gervaise in one pass**, respecting the PRD
while adhering to the Product Laws.

This system must remain **small, fast to iterate, and architecturally
honest**.

Do not introduce unnecessary complexity.

------------------------------------------------------------------------

# Core Product Rule (Critical)

Baby Gervaise must implement:

## ONE CONTINUOUS CHAT

There is:

• ONE persistent conversation\
• ONE chat interface\
• ONE continuous interaction with Gervaise

There are **no**:

• chat threads\
• conversations\
• assistants\
• sessions\
• message containers

The conversation **never resets**.

It persists across app restarts and across time.

Mental model:

> "I am speaking with Gervaise and the conversation continues."

This rule is **fundamental** because HGIE must evolve as a **continuous
interaction engine**, not a thread‑based chatbot.

------------------------------------------------------------------------

# Architecture Constraints

Keep the architecture extremely simple.

System structure:

Interface\
→ HGIE\
→ Memory\
→ Tools\
→ Models

Do NOT introduce:

• orchestrators\
• activity graphs\
• node routing\
• multi‑model logic\
• agent frameworks

These systems belong to future phases.

------------------------------------------------------------------------

# Target Platform

Primary platform:

Android

Technology stack:

Kotlin Android application\
Web‑based UI shell\
Rust core library

Rust library name:

baby_gervaise_core

Rust handles:

• HGIE\
• Memory\
• Model module\
• Tools interface

Android handles:

• UI\
• system integration\
• assistant entry point\
• voice input later

------------------------------------------------------------------------

# UI Requirements

Two screens only.

## Screen 1 --- Chat

Primary interface.

Features:

• message input\
• streaming responses\
• conversation history

This is the **single persistent conversation**.

No threads.

No "new chat".

Just **one continuous conversation with Gervaise**.

------------------------------------------------------------------------

## Screen 2 --- Overview

Accessible via a top‑right button.

This screen provides system diagnostics.

### Model Stats

• model name\
• request count\
• token usage\
• latency

### Memory Stats

• stored memories\
• vector count\
• retrieval activity

### System Stats

• total interactions\
• tool calls

------------------------------------------------------------------------

## Raw Model Logs

Overview must include a **log viewer** showing:

timestamp\
raw prompt\
raw model output

This is essential for HGIE experimentation.

------------------------------------------------------------------------

# Configuration System

Model provider must be configured through a build‑time config file.

Example:

config/model_config.json

Example structure:

{ "provider": "openai", "api_key": "YOUR_KEY", "model": "gpt-4o-mini",
"endpoint": "https://api.openai.com/v1", "temperature": 0.7 }

Baby Gervaise uses **ONE model only**.

No routing.

No fast/deep models.

Just one conversational model.

------------------------------------------------------------------------

# Memory System

Implement **simple RAG memory**.

Storage:

SQLite database.

Memory capabilities:

• store conversation summaries\
• store key facts\
• store embeddings\
• semantic retrieval

When HGIE builds prompts it may retrieve:

• recent conversation messages\
• semantic matches from memory

------------------------------------------------------------------------

# Context Retrieval Configuration

Add a user configuration setting:

Previous Context

Values:

Low\
Medium\
High

Behavior:

Low

• minimal memory retrieval\
• very small context window

Medium

• recent messages\
• moderate semantic retrieval

High

• deeper semantic retrieval\
• larger context window

This allows testing different memory behaviors.

------------------------------------------------------------------------

# Models Module

The model module must:

• call the configured API\
• track token usage\
• measure latency\
• log prompt and response

All model calls pass through this module.

------------------------------------------------------------------------

# Tools System

Implement a simple tools framework.

Tools represent **capabilities**.

Initial tools:

## Spotify

Possible actions:

• play music\
• pause playback\
• search songs\
• adjust volume

------------------------------------------------------------------------

## Philips Hue

Possible actions:

• turn lights on/off\
• adjust brightness\
• change colors\
• activate scenes

The LLM may **suggest tool usage**, but the **system executes tools
deterministically**.

------------------------------------------------------------------------

# Voice (Future Phase)

Voice will be added later.

Future pipeline:

Voice Input\
→ Speech‑to‑Text\
→ HGIE

Do not implement voice yet, but structure HGIE so voice can easily plug
in.

------------------------------------------------------------------------

# Persistence

Persist locally:

• conversation history\
• memory vectors\
• system statistics\
• configuration settings

Data must survive application restarts.

------------------------------------------------------------------------

# Project Structure

Generate the following structure:

/android\
/ui_web\
/rust_core\
/config

Rust modules:

hgie.rs\
memory.rs\
model.rs\
tools.rs\
logging.rs

Also include:

• SQLite schema\
• embedding storage\
• chat UI\
• overview diagnostics screen\
• log viewer\
• Spotify tool stub\
• Hue tool stub

------------------------------------------------------------------------

# Final Goal

The finished application must behave like a **small but real
assistant**.

The user opens the app and interacts with **one continuous Gervaise**.

HGIE interprets input, retrieves memory, calls the model, optionally
triggers tools, and returns a response.

This system should feel like:

**Baby Gervaise --- the first living prototype of the Gervaise
intelligence system.**
