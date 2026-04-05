// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Runtime-agnostic one-shot future for delivering file-picker results.
//!
//! [`PickFuture`] and [`PickState`] implement a simple single-value future
//! using only `std` primitives (`Arc`, `Mutex`, `Waker`).  This avoids any
//! dependency on Tokio, async-std, or other async runtimes, making the crate
//! usable from **any** executor — including `pollster::block_on`, Dioxus,
//! egui, Iced, and Xilem.
//!
//! # Usage (crate-internal)
//!
//! 1. Create a shared `Arc<Mutex<PickState<T>>>`.
//! 2. Return a `PickFuture<T>` wrapping that state to the caller.
//! 3. From the platform callback, call [`deliver`] with the result value.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Shared state between the [`PickFuture`] and the platform callback that
/// delivers the result.
// Used by non-desktop platforms (Android, iOS) that drive callbacks through
// `deliver()`.  On desktop the `rfd` crate's own async API is used instead.
#[allow(dead_code)]
pub(crate) struct PickState<T> {
    /// The result value, set exactly once by [`deliver`].
    pub result: Option<T>,
    /// The most recent [`Waker`] registered by the executor.
    pub waker: Option<Waker>,
}

/// A future that resolves to a single value of type `T`.
///
/// This is the core async primitive used by all platform picker
/// implementations to bridge callback-based native APIs into Rust futures.
// Used by non-desktop platforms; on desktop, `rfd`'s own async API suffices.
#[allow(dead_code)]
#[must_use = "futures do nothing unless polled"]
pub(crate) struct PickFuture<T> {
    /// Shared state with the callback side.
    pub state: Arc<Mutex<PickState<T>>>,
}

impl<T: Send + 'static> Future for PickFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        if let Some(value) = guard.result.take() {
            return Poll::Ready(value);
        }

        guard.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// Deliver a result value to a [`PickFuture`] and wake the executor.
///
/// This function is called from platform callbacks (JNI, Objective-C delegates,
/// JS event listeners, etc.) to complete the associated future.
///
/// # Panics
///
/// This function does not panic.  If the mutex is poisoned, it recovers the
/// inner state and proceeds normally.
// Used by non-desktop platforms; on desktop, `rfd`'s own async API suffices.
#[allow(dead_code)]
pub(crate) fn deliver<T>(state: &Arc<Mutex<PickState<T>>>, value: T) {
    let waker = {
        let mut guard = match state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.result = Some(value);
        guard.waker.take()
    };

    if let Some(w) = waker {
        w.wake();
    }
}

/// Create a new `(PickFuture<T>, Arc<Mutex<PickState<T>>>)` pair.
///
/// The returned `Arc` should be passed to the platform callback side, while
/// the `PickFuture` is returned to the caller to be awaited.
// Used by non-desktop platforms; on desktop, `rfd`'s own async API suffices.
#[allow(dead_code)]
pub(crate) fn new_pick_future<T>() -> (PickFuture<T>, Arc<Mutex<PickState<T>>>) {
    let state = Arc::new(Mutex::new(PickState {
        result: None,
        waker: None,
    }));
    let future = PickFuture {
        state: Arc::clone(&state),
    };
    (future, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{RawWaker, RawWakerVTable};

    /// Create a no-op waker for testing poll behaviour without an executor.
    fn noop_waker() -> Waker {
        fn noop(_: *const ()) {}
        fn clone(p: *const ()) -> RawWaker {
            RawWaker::new(p, &VTABLE)
        }
        const VTABLE: RawWakerVTable =
            RawWakerVTable::new(clone, noop, noop, noop);
        // SAFETY: The noop waker functions are trivially safe — they perform
        // no operations on the data pointer.
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    #[test]
    fn future_returns_pending_before_delivery() {
        let (mut future, _state) = new_pick_future::<i32>();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let pinned = Pin::new(&mut future);
        assert!(
            pinned.poll(&mut cx).is_pending(),
            "future should be Pending before deliver()"
        );
    }

    #[test]
    fn future_returns_ready_after_delivery() {
        let (future, state) = new_pick_future::<i32>();
        deliver(&state, 42);
        let result = pollster::block_on(future);
        assert_eq!(result, 42);
    }

    #[test]
    fn future_returns_ready_after_delivery_with_pollster() {
        let (future, state) = new_pick_future::<String>();
        deliver(&state, "hello".to_owned());
        let result = pollster::block_on(future);
        assert_eq!(result, "hello");
    }

    #[test]
    fn deliver_wakes_the_waker() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static WOKEN: AtomicBool = AtomicBool::new(false);

        fn noop(_: *const ()) {}
        fn wake(_: *const ()) {
            WOKEN.store(true, Ordering::SeqCst);
        }
        fn clone_fn(p: *const ()) -> RawWaker {
            RawWaker::new(p, &WAKE_VTABLE)
        }
        const WAKE_VTABLE: RawWakerVTable =
            RawWakerVTable::new(clone_fn, wake, wake, noop);

        WOKEN.store(false, Ordering::SeqCst);

        let (mut future, state) = new_pick_future::<i32>();

        // SAFETY: The custom waker functions are trivially safe.
        let waker =
            unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &WAKE_VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let _ = Pin::new(&mut future).poll(&mut cx);

        deliver(&state, 99);
        assert!(WOKEN.load(Ordering::SeqCst), "waker should have been called");
    }
}
