# ADR 0001: Runtime-Agnostic Future Implementation

## Status

Accepted

## Context

`loki-file-access` must present native file-picker dialogs and return the
result as a Rust `Future`.  The crate targets a wide range of consumers:
Dioxus, egui, Iced, Xilem, and applications using `pollster::block_on` for
synchronous contexts.

Each of these frameworks has its own executor (or uses none at all).  Tying
the crate to a specific async runtime would force every consumer to depend on
that runtime, even if they never use it elsewhere.

## Decision

Implement a minimal one-shot future (`PickFuture<T>`) using only `std`
primitives:

- `Arc<Mutex<PickState<T>>>` holds the shared state.
- `PickState<T>` contains an `Option<T>` for the result and an
  `Option<Waker>` for the executor's waker.
- The `Future` implementation checks for a result on each `poll`, stores the
  waker if pending, and returns `Poll::Ready` once a result is delivered.
- A `deliver()` function is called from the platform callback (JNI, Objective-C
  delegate, JS event listener) to set the result and wake the executor.

No Tokio, async-std, or other runtime crate is required.

## Consequences

### Positive

- **Universal compatibility**: Works with any executor, including
  `pollster::block_on`, Tokio, async-std, smol, and custom executors.
- **Zero runtime dependencies**: The crate's dependency footprint is minimal.
- **Simple mental model**: One future, one result, one waker — easy to audit.

### Negative

- **Slightly more code** than using `tokio::sync::oneshot` or a similar
  runtime-provided channel.
- **No built-in timeout**: Consumers must implement their own timeout logic
  if needed (e.g. `tokio::time::timeout` wrapping the future).
- **Mutex overhead**: Each `poll` and `deliver` call acquires a mutex lock.
  This is negligible for file-picker operations (user-driven, infrequent).

## Alternatives Rejected

| Alternative | Reason for rejection |
|---|---|
| `tokio::sync::oneshot` | Adds a mandatory Tokio dependency. Consumers using other runtimes or `pollster` would be forced to pull in Tokio. |
| `async_channel` | Additional dependency for a single-use channel. The crate only needs one-shot delivery, not a full channel. |
| `flume` | Additional dependency. Same reasoning as `async_channel` — the problem is simpler than what `flume` solves. |
| `futures::channel::oneshot` | Adds the `futures` crate as a dependency. While lighter than Tokio, it is still an unnecessary dependency for this use case. |
