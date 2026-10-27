//! When an API call gets a 401, renew the session and send the call again.
//!
//! On a 401 the UI used to refresh the session and then reload the page.
//! That threw away scroll position, open dialogs and any edit that only a
//! draft protected. A failed refresh of any kind, even a network blip, sent
//! the user to the login page. And every call that got a 401 at the same
//! moment started its own refresh. The server rotates the refresh token, so
//! the second of two racing refreshes could be refused and log the user out.
//!
//! The UI's API client now goes through [`RefreshGate`]: one refresh at a
//! time, shared by every call that got a 401 meanwhile, then each call
//! re-sends once. The rules are pure, so they live here, where they are
//! tested on the host.
//!
//! Re-sending is safe for writes too. The API rejects an unauthenticated
//! request in its auth middleware, before any handler runs, so the first
//! attempt changed nothing.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

/// A refresh with no answer after this long is abandoned and counts as
/// [`RefreshOutcome::Unavailable`]. While one refresh is in flight every
/// other 401 waits for it, so a hung refresh must not hold them forever.
pub const REFRESH_TIMEOUT_MS: u32 = 10_000;

/// How a session refresh went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// New session cookies are set: re-send the call.
    Refreshed,
    /// The server refused the refresh token (401/403): the session is over.
    Rejected,
    /// The refresh didn't get an answer (network error, 5xx): nothing is
    /// known about the session yet, so this is not a logout.
    Unavailable,
}

/// Classify the refresh response. `None` is a request that never got a
/// response. Only a 401 or 403 is a verdict on the session, the same rule
/// AuthGate follows for `/auth/me`.
pub fn refresh_outcome(status: Option<u16>) -> RefreshOutcome {
    match status {
        Some(s) if (200..300).contains(&s) => RefreshOutcome::Refreshed,
        Some(401 | 403) => RefreshOutcome::Rejected,
        _ => RefreshOutcome::Unavailable,
    }
}

/// What the API client does with a call that got a 401.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Send the call again, once.
    Retry,
    /// Send the user to the login page.
    Login,
    /// Report an ordinary error: the caller shows it and offers its retry.
    Fail,
}

/// The step after a refresh.
pub fn after_refresh(outcome: RefreshOutcome) -> Next {
    match outcome {
        RefreshOutcome::Refreshed => Next::Retry,
        RefreshOutcome::Rejected => Next::Login,
        RefreshOutcome::Unavailable => Next::Fail,
    }
}

/// The step after the one retry got `status`. A second 401 on fresh
/// cookies means the session can't be renewed, so the user logs in again.
/// Anything else is the call's real answer. There is never a second retry.
pub fn after_retry(status: u16) -> Option<Next> {
    (status == 401).then_some(Next::Login)
}

/// One session refresh at a time, shared by every call that needs it.
///
/// `generation` counts successful refreshes. A call records it before
/// sending; if a refresh finished while the call was in flight, the call's
/// 401 came from the old cookie, so it re-sends without refreshing again.
/// Clones share state (single-threaded, like the WASM UI).
#[derive(Debug, Clone, Default)]
pub struct RefreshGate {
    state: Rc<RefCell<GateState>>,
}

#[derive(Debug, Default)]
struct GateState {
    generation: u64,
    in_flight: Option<Rc<RefCell<Slot>>>,
}

#[derive(Debug, Default)]
struct Slot {
    outcome: Option<RefreshOutcome>,
    waiters: Vec<Waker>,
}

impl RefreshGate {
    /// Successful refreshes so far. Read it before sending a call.
    pub fn generation(&self) -> u64 {
        self.state.borrow().generation
    }

    /// Renew the session for a call sent at generation `sent`. The first
    /// caller runs `refresh`; callers that arrive while it runs wait for its
    /// outcome instead of starting their own.
    pub async fn refresh<F, Fut>(&self, sent: u64, refresh: F) -> RefreshOutcome
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = RefreshOutcome>,
    {
        let joined = {
            let mut st = self.state.borrow_mut();
            if st.generation > sent {
                // Renewed while this call was in flight: just re-send.
                return RefreshOutcome::Refreshed;
            }
            match &st.in_flight {
                Some(slot) => Some(Rc::clone(slot)),
                None => {
                    st.in_flight = Some(Rc::new(RefCell::new(Slot::default())));
                    None
                }
            }
        };
        if let Some(slot) = joined {
            return Wait(slot).await;
        }
        // This call leads. The guard settles the slot even if this future is
        // dropped mid-refresh, so the waiters never hang.
        let mut lead = Lead {
            gate: self,
            outcome: RefreshOutcome::Unavailable,
        };
        lead.outcome = refresh().await;
        lead.outcome
    }
}

/// Settles the in-flight slot when the leading call finishes or is dropped.
struct Lead<'a> {
    gate: &'a RefreshGate,
    outcome: RefreshOutcome,
}

impl Drop for Lead<'_> {
    fn drop(&mut self) {
        let slot = {
            let mut st = self.gate.state.borrow_mut();
            if self.outcome == RefreshOutcome::Refreshed {
                st.generation = st.generation.wrapping_add(1);
            }
            st.in_flight.take()
        };
        if let Some(slot) = slot {
            let waiters = {
                let mut s = slot.borrow_mut();
                s.outcome = Some(self.outcome);
                std::mem::take(&mut s.waiters)
            };
            waiters.into_iter().for_each(Waker::wake);
        }
    }
}

/// Waits for the leading call's outcome.
struct Wait(Rc<RefCell<Slot>>);

impl Future for Wait {
    type Output = RefreshOutcome;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<RefreshOutcome> {
        let mut slot = self.0.borrow_mut();
        match slot.outcome {
            Some(o) => Poll::Ready(o),
            None => {
                slot.waiters.push(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::pin::pin;

    #[test]
    fn only_401_and_403_end_the_session() {
        assert_eq!(refresh_outcome(Some(200)), RefreshOutcome::Refreshed);
        assert_eq!(refresh_outcome(Some(204)), RefreshOutcome::Refreshed);
        assert_eq!(refresh_outcome(Some(401)), RefreshOutcome::Rejected);
        assert_eq!(refresh_outcome(Some(403)), RefreshOutcome::Rejected);
        // A blip is not a logout (the old client sent these to login).
        assert_eq!(refresh_outcome(Some(500)), RefreshOutcome::Unavailable);
        assert_eq!(refresh_outcome(Some(503)), RefreshOutcome::Unavailable);
        assert_eq!(refresh_outcome(Some(429)), RefreshOutcome::Unavailable);
        assert_eq!(refresh_outcome(None), RefreshOutcome::Unavailable);
    }

    #[test]
    fn a_hung_refresh_fails_before_the_editor_gives_up_on_a_save() {
        // The save then reports its own error (with Retry) instead of the
        // watchdog's "took too long", and other 401s stop waiting.
        use crate::save_watch::{GIVE_UP_AFTER_MS, STALL_AFTER_MS};
        const { assert!(STALL_AFTER_MS < REFRESH_TIMEOUT_MS) };
        const { assert!(REFRESH_TIMEOUT_MS < GIVE_UP_AFTER_MS) };
    }

    #[test]
    fn each_refresh_outcome_has_one_next_step() {
        assert_eq!(after_refresh(RefreshOutcome::Refreshed), Next::Retry);
        assert_eq!(after_refresh(RefreshOutcome::Rejected), Next::Login);
        assert_eq!(after_refresh(RefreshOutcome::Unavailable), Next::Fail);
    }

    #[test]
    fn a_retry_is_never_retried() {
        assert_eq!(after_retry(401), Some(Next::Login));
        for s in [200, 204, 404, 409, 412, 500] {
            assert_eq!(after_retry(s), None, "{s} is the call's answer");
        }
    }

    /// A refresh the test settles by hand; counts how often it ran.
    #[derive(Clone, Default)]
    struct Manual {
        outcome: Rc<Cell<Option<RefreshOutcome>>>,
        runs: Rc<Cell<u32>>,
    }

    impl Manual {
        fn run(&self) -> impl Future<Output = RefreshOutcome> + use<> {
            self.runs.set(self.runs.get() + 1);
            let outcome = Rc::clone(&self.outcome);
            std::future::poll_fn(move |_| match outcome.get() {
                Some(o) => Poll::Ready(o),
                None => Poll::Pending,
            })
        }
    }

    fn poll<F: Future>(f: Pin<&mut F>) -> Poll<F::Output> {
        f.poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn concurrent_401s_share_one_refresh() {
        let gate = RefreshGate::default();
        let r = Manual::default();
        let sent = gate.generation();
        let mut a = pin!(gate.refresh(sent, || r.run()));
        let mut b = pin!(gate.refresh(sent, || r.run()));
        assert!(poll(a.as_mut()).is_pending());
        assert!(poll(b.as_mut()).is_pending());
        assert_eq!(r.runs.get(), 1, "the second call waits, it doesn't refresh");

        r.outcome.set(Some(RefreshOutcome::Refreshed));
        assert_eq!(poll(a.as_mut()), Poll::Ready(RefreshOutcome::Refreshed));
        assert_eq!(poll(b.as_mut()), Poll::Ready(RefreshOutcome::Refreshed));
        assert_eq!(gate.generation(), sent + 1);
    }

    #[test]
    fn waiters_share_a_rejection_too() {
        let gate = RefreshGate::default();
        let r = Manual::default();
        let mut a = pin!(gate.refresh(0, || r.run()));
        let mut b = pin!(gate.refresh(0, || r.run()));
        assert!(poll(a.as_mut()).is_pending());
        assert!(poll(b.as_mut()).is_pending());
        r.outcome.set(Some(RefreshOutcome::Rejected));
        assert_eq!(poll(a.as_mut()), Poll::Ready(RefreshOutcome::Rejected));
        assert_eq!(poll(b.as_mut()), Poll::Ready(RefreshOutcome::Rejected));
        assert_eq!(gate.generation(), 0, "only a success counts");
    }

    #[test]
    fn a_call_sent_before_a_finished_refresh_just_resends() {
        let gate = RefreshGate::default();
        let r = Manual::default();
        let sent_early = gate.generation();
        r.outcome.set(Some(RefreshOutcome::Refreshed));
        let done = poll(pin!(gate.refresh(sent_early, || r.run())));
        assert_eq!(done, Poll::Ready(RefreshOutcome::Refreshed));
        assert_eq!(r.runs.get(), 1);

        // Its 401 came from the old cookie: re-send, no second refresh.
        let late = poll(pin!(gate.refresh(sent_early, || r.run())));
        assert_eq!(late, Poll::Ready(RefreshOutcome::Refreshed));
        assert_eq!(r.runs.get(), 1);
    }

    #[test]
    fn a_401_on_the_new_cookie_refreshes_again() {
        let gate = RefreshGate::default();
        let r = Manual::default();
        r.outcome.set(Some(RefreshOutcome::Refreshed));
        let _ = poll(pin!(gate.refresh(0, || r.run())));
        // Sent after that refresh, still 401 (it expired again later).
        let sent = gate.generation();
        let _ = poll(pin!(gate.refresh(sent, || r.run())));
        assert_eq!(r.runs.get(), 2);
        assert_eq!(gate.generation(), 2);
    }

    #[test]
    fn a_dropped_leader_releases_its_waiters() {
        let gate = RefreshGate::default();
        let r = Manual::default();
        let mut b;
        {
            let mut a = Box::pin(gate.refresh(0, || r.run()));
            assert!(poll(a.as_mut()).is_pending());
            b = Box::pin(gate.refresh(0, || r.run()));
            assert!(poll(b.as_mut()).is_pending());
        } // `a` is dropped mid-refresh.
        assert_eq!(poll(b.as_mut()), Poll::Ready(RefreshOutcome::Unavailable));
        // The gate is free again: the next 401 starts a fresh refresh.
        r.outcome.set(Some(RefreshOutcome::Refreshed));
        let next = poll(pin!(gate.refresh(0, || r.run())));
        assert_eq!(next, Poll::Ready(RefreshOutcome::Refreshed));
        assert_eq!(r.runs.get(), 2);
    }
}
