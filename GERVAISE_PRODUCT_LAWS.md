
# Gervaise Product Laws

Version: v0.1
Status: Foundational Product Doctrine
Date: 2026

---

## Purpose

This document defines the **non‑negotiable product laws of Gervaise**.

These laws protect the identity of the system as it evolves and ensure that Gervaise never drifts into becoming:

- a chatbot
- a prompt engineering tool
- an agent wrapper
- a plugin marketplace
- a multi‑thread chat interface
- a super‑app clone

Instead, Gervaise must remain an **AI‑native computing system** built around one continuous intelligence.

---

## Core Philosophy

Gervaise follows a simple runtime model:

Human Interaction → HGIE → Activity Graph → Orchestrator → Execution

Human interaction is interpreted by the system, formalized into structured work, and executed across devices.

The user experiences **one continuous intelligence** while the system internally manages structured activities.

---

## Law 1 — One Continuous Interface

Gervaise exposes **one interaction surface**.

There are:

- no chat threads
- no assistant instances
- no conversation containers
- no session switching

The user interacts with **one persistent Gervaise**.

Correct mental model:

> I am interacting with Gervaise and it continues the work.

---

## Law 2 — Activities Are the System

The primary unit of work in Gervaise is the **Activity**.

User input is interpreted and transformed into **activity candidates**, which become structured activities.

Activities may:

- spawn child activities
- depend on other activities
- run in parallel
- persist across time
- move across devices

Chat history is not the system.

The **Activity Graph** is the system.

---

## Law 3 — The Interface Is Not the Architecture

The chat interface is only a **communication layer**.

Internally the system operates on:

- activities
- operations
- entities
- artifacts
- memory structures

The interface must remain simple while the runtime remains structured.

---

## Law 4 — Gervaise Is the Environment

Gervaise is not primarily a launcher for other apps.

Instead of:

User → App → Another App

The interaction becomes:

User → Gervaise

External systems may still be used, but the user experience remains centered on Gervaise.

---

## Law 5 — Capabilities, Not Apps

External systems integrate as **capabilities**, not applications.

Examples include:

- Spotify
- Hue
- Slack
- Filesystem
- IDEs

These integrations are invoked through system operations.

---

## Law 6 — Models Are Invisible

Users should not need to understand AI models.

The runtime internally manages model slots such as:

- fast
- deep

The orchestrator automatically selects the appropriate model.

Power users may configure models, but this is optional.

---

## Law 7 — Complexity Must Stay Inside the System

The system may be technically sophisticated, but complexity must remain internal.

Users should experience:

- continuity
- clarity
- calm interaction
- predictable behavior

---

## Law 8 — Local First, Distributed by Design

Gervaise is **local‑first**.

Every node must remain useful offline.

Nodes can synchronize to form a trusted personal mesh.

Cloud infrastructure may exist but must never become the mandatory center of the system.

---

## Law 9 — Deterministic Control

LLMs assist interpretation but must **not control the system**.

Deterministic components include:

- orchestration
- scheduling
- policy enforcement
- execution control

System decisions must remain explainable.

---

## Law 10 — Power Without Complexity

Gervaise must support both:

- casual users
- advanced users

Advanced capabilities may include:

- custom integrations
- developer tooling
- external model APIs

But these must remain optional and must never complicate the default experience.

---

## One Sentence Definition

Gervaise is a **local‑first distributed intelligence system** where a human interacts with one continuous computational partner that turns natural interaction into structured activities executed across devices.
