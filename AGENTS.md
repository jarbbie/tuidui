# AGENTS.md

## Project Overview

**tuidui** is a terminal-first social and community platform written in Rust.

The long-term idea is a mix of:

- Discord-style servers, channels, realtime chat, presence, and voice rooms
- Facebook-style profiles, friends, posts, comments, likes, and notifications
- A native terminal UI instead of a browser frontend

The project is primarily a learning project for backend engineering, async Rust, networking, realtime systems, databases, and eventually distributed systems.

The goal is **not** to clone Discord or Facebook feature-for-feature.

Build the system incrementally and keep every milestone usable.

---

# Tech Stack

## TUI Client

- Rust
- `ratatui`
- `crossterm`
- `tokio`
- `reqwest`
- WebSocket client
- `ratatui-image` for avatars/images where supported

## Backend

- Rust
- `axum`
- `tokio`
- `serde`
- `sqlx`
- PostgreSQL

## Shared

Shared Rust types should be used where appropriate for:

- API DTOs
- IDs
- enums
- WebSocket events
- protocol definitions

Do not share database-specific models with the client.

---

# Architecture

Start as a **modular monolith**.

Do not introduce microservices unless there is a concrete technical reason later.

Desired high-level structure:

```text
termhub/
├── Cargo.toml
├── crates/
│   ├── tui/
│   │   └── src/
│   ├── server/
│   │   └── src/
│   ├── api-types/
│   │   └── src/
│   └── app-core/
│       └── src/
├── migrations/
├── AGENTS.md
└── README.md
```

Conceptually:

```text
┌───────────────────┐
│   Ratatui Client  │
│                   │
│ UI                │
│ local state       │
│ API client        │
└─────────┬─────────┘
          │
     HTTP / WebSocket
          │
┌─────────▼─────────┐
│    Axum Server    │
│                   │
│ routes            │
│ websocket         │
│ auth              │
│ business logic    │
└─────────┬─────────┘
          │
         SQLx
          │
┌─────────▼─────────┐
│    PostgreSQL     │
└───────────────────┘
```

---

# Development Philosophy

## 1. Build vertically

Prefer complete end-to-end features over partially implementing many systems.

Good:

```text
database
↓
repository
↓
service
↓
HTTP endpoint
↓
client request
↓
TUI screen
```

Bad:

```text
create 30 database tables
create 40 API routes
design 15 TUI screens
finish none of them
```

Every major feature should become usable before moving to the next major feature.

---

## 2. Do not overengineer

Do not add infrastructure simply because large systems such as Discord use it.

Avoid introducing these without a demonstrated need:

- Redis
- Kafka
- NATS
- RabbitMQ
- Kubernetes
- microservices
- distributed databases
- event sourcing
- CQRS
- complex caching layers

Initially, this is enough:

```text
TUI
 ↓
Axum
 ↓
PostgreSQL
```

WebSockets may maintain in-memory connection state in the server initially.

Scaling problems should be encountered, measured, understood, and then solved.

---

## 3. Learning matters more than abstraction

Do not hide important backend concepts behind large frameworks.

Prefer implementations that expose concepts such as:

- HTTP request lifecycle
- middleware
- authentication
- authorization
- SQL queries
- transactions
- WebSocket lifecycle
- async tasks
- channels
- connection management
- concurrency
- state synchronization

Libraries are welcome when they remove irrelevant boilerplate.

They should not eliminate the concepts this project exists to teach.

---

# Feature Roadmap

Do not attempt all of these simultaneously.

## Phase 0 — TUI

Implement a local TUI with fake data.

Learn:

- Ratatui layouts
- widgets
- keyboard events
- application state
- navigation
- terminal lifecycle

Suggested layout:

```text
┌──────────────┬────────────────────────────┐
│ Servers      │ # general                  │
│              │                            │
│ rust         │ alice: hello               │
│ kmutt        │ bob: hey                   │
│ gaming       │                            │
│              │                            │
│              │                            │
├──────────────┴────────────────────────────┤
│ Message: _                               │
└───────────────────────────────────────────┘
```

---

## Phase 1 — REST Backend

Implement:

- users
- servers
- channels
- messages

Initial endpoints can include:

```text
GET    /health

GET    /servers
POST   /servers

GET    /servers/:server_id/channels

GET    /channels/:channel_id/messages
POST   /channels/:channel_id/messages
```

The TUI should fetch real data from the Axum server.

---

## Phase 2 — Authentication

Implement:

- registration
- login
- logout
- password hashing
- authenticated requests
- authorization checks

Clearly separate:

```text
Authentication
Who are you?

Authorization
What are you allowed to do?
```

Never trust authorization decisions made by the client.

The server is authoritative.

---

## Phase 3 — WebSocket Chat

Add a persistent WebSocket connection.

Use REST for state retrieval and normal commands where appropriate.

Use WebSockets primarily for realtime events.

Example server events:

```rust
enum ServerEvent {
    MessageCreated(MessageDto),
    MessageUpdated(MessageDto),
    MessageDeleted {
        message_id: MessageId,
    },
    MemberJoined {
        server_id: ServerId,
        user: UserDto,
    },
    MemberLeft {
        server_id: ServerId,
        user_id: UserId,
    },
    TypingStarted {
        channel_id: ChannelId,
        user_id: UserId,
    },
}
```

Do not poll for realtime chat once WebSockets are implemented.

---

## Phase 4 — Social Features

Add incrementally:

- profiles
- friend requests
- friendships
- blocking
- direct messages
- posts
- comments
- likes
- notifications

Do not implement a sophisticated recommendation algorithm initially.

A chronological feed is enough.

---

## Phase 5 — Presence

Implement:

- online
- offline
- optionally idle
- last seen
- heartbeats

Presence should account for abnormal disconnects.

Do not assume every client closes its connection cleanly.

---

## Phase 6 — Voice

Voice chat is intentionally late in the roadmap.

Start with:

- join voice room
- leave voice room
- mute
- unmute
- push-to-talk
- participant list

Then explore:

- audio capture
- playback
- encoding
- packetization
- latency
- jitter
- packet loss

Later research may include:

- UDP
- RTP
- WebRTC
- ICE
- STUN
- TURN
- NAT traversal

Do not implement voice by blindly sending huge raw audio buffers through the existing chat WebSocket.

Voice transport should eventually be treated as its own networking problem.

---

# TUI Architecture

Avoid mixing rendering, input handling, and network operations.

Prefer an event/update/render architecture.

Conceptually:

```text
terminal/network event
        ↓
     Message
        ↓
     update()
        ↓
    App State
        ↓
     render()
```

Example:

```rust
enum Message {
    Key(KeyEvent),

    ServersLoaded(Vec<ServerDto>),
    MessagesLoaded(Vec<MessageDto>),

    ChatMessageReceived(MessageDto),

    AvatarLoaded {
        user_id: UserId,
        avatar: Avatar,
    },

    Error(String),
}
```

Rendering functions should primarily render state.

Do not make HTTP requests from rendering functions.

Bad:

```rust
fn draw(frame: &mut Frame) {
    let users = client.get_users().await;
}
```

Good:

```text
network task
   ↓
Message::UsersLoaded
   ↓
update app state
   ↓
draw current state
```

---

# Async Work

The TUI must remain responsive while network operations occur.

Potential long-running operations should run asynchronously.

Examples:

- HTTP requests
- WebSocket receive loop
- image downloads
- avatar decoding
- future file uploads

Communicate results back to application state through events/messages.

Avoid blocking the terminal event loop.

---

# REST vs WebSocket

As a general guideline:

## REST

Use REST for operations such as:

```text
register
login

load profile
edit profile

create server
create channel

fetch message history
fetch friends

create posts
load feed
```

## WebSocket

Use WebSockets for events such as:

```text
new message

message edited
message deleted

user online
user offline

typing started

friend request received

notification received

member joined server
```

This is a guideline, not an absolute rule.

Choose the protocol that makes the semantics clear.

---

# Database Guidelines

Use PostgreSQL through SQLx.

Prefer explicit SQL over unnecessarily abstract ORM behavior.

Use migrations for schema changes.

Never manually modify production-like database schema outside migrations.

Potential tables include:

```text
users

servers
server_members
channels

messages

friendships

posts
comments
likes

notifications
```

Do not create every future table immediately.

Only add schema required by the current feature.

---

# Database Transactions

Use transactions when multiple database operations must succeed or fail together.

Example:

Accepting a friend request may eventually require:

```text
BEGIN

verify pending request

update friendship state

create notification

COMMIT
```

Think explicitly about:

- partial failure
- concurrent requests
- duplicate operations
- idempotency

Do not use a transaction merely because multiple SQL statements exist.

Use one when atomicity is required.

---

# Data Models

Keep these concepts separate when appropriate:

```text
Database Model
API DTO
Domain Model
TUI State
```

For example:

```rust
struct UserRow {
    id: Uuid,
    password_hash: String,
    created_at: DateTime<Utc>,
}
```

must never accidentally become the API response.

Instead expose something like:

```rust
struct UserDto {
    id: UserId,
    username: String,
    display_name: String,
    avatar_url: Option<String>,
}
```

Never expose:

- password hashes
- internal secrets
- session secrets
- authorization-sensitive internal fields

---

# Shared Crate

The `shared` crate may contain:

```text
DTOs
IDs
public enums
request/response types
WebSocket protocol definitions
```

Example:

```rust
pub struct UserId(pub Uuid);

pub struct ChannelId(pub Uuid);
```

Prefer typed IDs where they improve safety.

Do not turn `shared` into a dumping ground.

It should not contain:

- SQLx repositories
- Axum handlers
- TUI widgets
- database connection logic

---

# Error Handling

Avoid `.unwrap()` in normal runtime paths.

Prefer meaningful error propagation.

Server errors should eventually map into appropriate HTTP responses.

Examples:

```text
400 Bad Request
401 Unauthorized
403 Forbidden
404 Not Found
409 Conflict
500 Internal Server Error
```

Do not expose internal database errors directly to clients.

Logs may contain detailed diagnostic information.

Client-facing errors should be understandable and safe.

---

# Logging

Use structured logging where practical.

Prefer:

```rust
tracing
tracing-subscriber
```

Useful information may include:

```text
request method
route
request ID
user ID when appropriate
duration
error category
```

Never log:

- passwords
- authentication tokens
- private keys
- session secrets

---

# Authentication

Password storage must use a proper password hashing algorithm such as Argon2.

Never store plaintext passwords.

Do not invent custom cryptography.

Authentication architecture should stay simple initially.

Possible approaches:

- secure session tokens
- JWT if there is a clear reason

Do not choose JWT merely because it is common.

---

# Authorization

Every sensitive operation must be authorized server-side.

Examples:

Before deleting a message:

```text
Is the user:

- the message author?
- or a moderator with sufficient permission?
```

Before loading a private channel:

```text
Is the requesting user actually a member?
```

Never rely on hiding UI controls as authorization.

The TUI is untrusted from the server's perspective.

---

# Images and Avatars

The TUI may support real images using `ratatui-image`.

Potential terminal protocols include:

- Kitty Graphics Protocol
- Sixel
- iTerm2 graphics
- Unicode half-block fallback

Avatar loading should be asynchronous.

Do not download or decode avatars inside `draw()`.

Preferred flow:

```text
profile loaded
    ↓
avatar cache checked
    ↓
async image download
    ↓
decode/resize
    ↓
AvatarLoaded event
    ↓
store rendered/cached avatar
    ↓
next frame displays it
```

Cache avatars to avoid repeated network requests and decoding.

The application must remain usable without image protocol support.

---

# Realtime Architecture

Initially, an in-process connection manager is acceptable.

For example:

```text
Axum
 ├── connection Alice
 ├── connection Bob
 └── connection Charlie
```

Tokio primitives may include:

```rust
mpsc
broadcast
oneshot
watch
```

Use them intentionally.

Do not put the entire server state behind one giant:

```rust
Arc<RwLock<HashMap<...>>>
```

unless the scope is small and the tradeoff is understood.

Prefer clear ownership and message passing where appropriate.

---

# Performance

Do not optimize without evidence.

Before optimizing:

1. identify the suspected bottleneck
2. measure it
3. understand why it occurs
4. change the architecture only if necessary
5. measure again

Potential future bottlenecks may include:

- database queries
- WebSocket fan-out
- message history
- avatar loading
- large servers
- presence tracking
- voice traffic

Do not prematurely solve hypothetical scale.

---

# Pagination

Never assume potentially large lists can always be loaded entirely.

Message history should eventually support cursor-based pagination.

Conceptually:

```text
GET /channels/:id/messages?before=<message_id>&limit=50
```

The TUI can request older messages when the user scrolls upward.

---

# Testing

Prefer testing business behavior rather than implementation details.

Useful categories:

## Unit tests

For pure logic such as:

```text
permission calculation
friendship state transitions
validation
protocol parsing
```

## Integration tests

For:

```text
database repositories
HTTP endpoints
authentication
transactions
```

## End-to-end tests

Later, test important complete flows such as:

```text
register
→ login
→ create server
→ create channel
→ send message
→ second client receives message
```

---

# Code Style

Follow standard Rust conventions.

Before considering work complete, run:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo test --workspace
```

Prefer code that is:

- explicit
- readable
- idiomatic
- testable
- easy to modify

Avoid unnecessary abstraction.

Three duplicated lines are sometimes better than a premature generic framework.

---

# Dependency Policy

Before adding a dependency, ask:

1. What problem does it solve?
2. Can the standard library/current stack reasonably solve it?
3. Is the crate maintained?
4. Is it introducing a large abstraction for a small problem?
5. Does it hide something this project intends to learn?

Do not add dependencies merely for convenience without considering these questions.

---

# Agent Behavior

When working on this repository:

## Do

- inspect existing architecture before making changes
- preserve established conventions
- explain significant architectural decisions
- implement the smallest complete solution
- keep changes scoped to the requested feature
- reuse existing abstractions when they are appropriate
- add tests for important behavior
- update migrations when schema changes
- consider failure and concurrent behavior
- keep client and server responsibilities separate

## Do Not

- rewrite unrelated code
- introduce microservices without being asked
- add Redis/Kafka/etc. preemptively
- invent unnecessary abstractions
- silently change API contracts
- place secrets in source code
- bypass authentication or authorization
- block Tokio threads with long synchronous operations
- perform networking inside TUI render functions
- share database internals with the client
- implement future roadmap features unless required

---

# When Unsure

Prefer the simplest architecture that:

1. works correctly
2. teaches the underlying concept clearly
3. can reasonably evolve later

The intended progression of the project is:

```text
TUI
 ↓
REST
 ↓
Database
 ↓
Authentication
 ↓
Authorization
 ↓
WebSocket
 ↓
Social graph
 ↓
Presence
 ↓
Voice
 ↓
Scaling
 ↓
Distributed systems
```

Do not skip ahead simply because the later architecture looks more impressive.

The complexity of tuidui should grow because the product requires it, not because we predicted it might someday be necessary.

---

# Core Principle

> Build the smallest real system first, then let real limitations teach us what architecture comes next.
