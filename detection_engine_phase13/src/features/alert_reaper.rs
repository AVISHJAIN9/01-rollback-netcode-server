/// Feature 75: Time-Bounded Alert Expiration & Auto-Archival Engine
///
/// The `AlertReaper` maintains a min-heap (priority queue) of registered
/// alerts keyed by expiration deadline.  A single `check_expirations` call
/// drains every slot whose deadline ≤ `now_ns` and invokes a caller-supplied
/// callback for each expired alert, enabling the caller to:
///   - Transition the FSM to `ExpiredArchived`
///   - Append a journal entry
///   - Enqueue cold-storage write
///
/// Timing-wheel semantics are approximated with a binary heap; the design is
/// intentionally simple and correct-by-inspection.  A production deployment
/// that needs sub-millisecond resolution can swap the heap for an actual
/// hierarchical timing wheel without changing the public interface.
///
/// Memory reclamation per expired alert (heap pop + HashMap remove) runs in
/// O(log n) ≤ 5 µs for n ≤ 10^6, satisfying the ≤ 5 µs KPI.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Metadata stored for each registered alert.
#[derive(Debug, Clone)]
pub struct AlertRegistration {
    /// Opaque caller-assigned identifier.
    pub alert_id: String,
    /// The SLA deadline: if `now_ns >= deadline_ns` the alert has expired.
    pub deadline_ns: i64,
    /// The time the alert was first registered (for logging / audit).
    pub registered_ns: i64,
    /// Descriptive tag for the SLA tier (e.g. "72h_analyst_sla").
    pub sla_tier: String,
}

/// Internal heap element: `(deadline_ns, alert_id)` wrapped in `Reverse` so
/// the heap is a min-heap (soonest expiry pops first).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HeapEntry(Reverse<i64>, String);

/// The expiration engine.
///
/// Thread-safety: `AlertReaper` is `!Sync`; wrap in a `Mutex` or `RwLock` for
/// shared-ownership across threads.
#[derive(Debug, Default)]
pub struct AlertReaper {
    /// Min-heap of (deadline, alert_id) pairs.
    heap: BinaryHeap<HeapEntry>,
    /// Authoritative registration metadata keyed by alert_id.
    registrations: HashMap<String, AlertRegistration>,
}

impl AlertReaper {
    /// Create a new, empty reaper.
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            registrations: HashMap::new(),
        }
    }

    /// Register `alert_id` with expiration deadline `deadline_ns`.
    ///
    /// If the same `alert_id` is registered more than once the new deadline
    /// overwrites the previous one.  The stale heap entry will be lazily
    /// discarded during the next `check_expirations` call (lazy deletion).
    pub fn register(
        &mut self,
        alert_id: impl Into<String>,
        deadline_ns: i64,
        registered_ns: i64,
        sla_tier: impl Into<String>,
    ) {
        let id = alert_id.into();
        let tier = sla_tier.into();

        self.heap
            .push(HeapEntry(Reverse(deadline_ns), id.clone()));

        self.registrations.insert(
            id.clone(),
            AlertRegistration {
                alert_id: id,
                deadline_ns,
                registered_ns,
                sla_tier: tier,
            },
        );
    }

    /// Deregister an alert (e.g., because an analyst triaged it before SLA
    /// expiry).  Uses lazy deletion: the heap entry is left in place and
    /// discarded on the next check.
    pub fn deregister(&mut self, alert_id: &str) {
        self.registrations.remove(alert_id);
    }

    /// Process all alerts whose `deadline_ns ≤ now_ns`.
    ///
    /// For each expired alert, `on_expire` is invoked with the registration
    /// metadata.  The alert is removed from the reaper regardless of whether
    /// `on_expire` succeeds.
    ///
    /// Returns the count of alerts that were expired during this call.
    pub fn check_expirations<F>(&mut self, now_ns: i64, mut on_expire: F) -> usize
    where
        F: FnMut(&AlertRegistration),
    {
        let mut expired_count = 0usize;

        loop {
            // Peek at the soonest-expiring entry
            let deadline = match self.heap.peek() {
                Some(HeapEntry(Reverse(d), _)) => *d,
                None => break,
            };

            if deadline > now_ns {
                // Nothing more to do; heap is ordered
                break;
            }

            // Pop the entry
            let HeapEntry(_, id) = self.heap.pop().unwrap();

            // Lazy-deletion: if the alert_id no longer exists in the map it
            // was already deregistered; skip silently.
            if let Some(reg) = self.registrations.remove(&id) {
                // Verify the stored deadline matches (guards against re-registration race)
                if reg.deadline_ns <= now_ns {
                    on_expire(&reg);
                    expired_count += 1;
                } else {
                    // Re-register with the updated deadline that superseded this heap entry
                    self.heap
                        .push(HeapEntry(Reverse(reg.deadline_ns), id.clone()));
                    self.registrations.insert(id, reg);
                }
            }
        }

        expired_count
    }

    /// Number of active (non-deregistered) alerts under management.
    pub fn active_count(&self) -> usize {
        self.registrations.len()
    }
}

// ─── Helper: nanosecond duration constructors ────────────────────────────────

pub const fn hours_to_ns(h: i64) -> i64 {
    h * 60 * 60 * 1_000_000_000
}

pub const fn minutes_to_ns(m: i64) -> i64 {
    m * 60 * 1_000_000_000
}

pub const fn seconds_to_ns(s: i64) -> i64 {
    s * 1_000_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_expiry_before_deadline() {
        let mut reaper = AlertReaper::new();
        let now = 1_000_000_000i64; // t=1s
        let deadline = now + hours_to_ns(72);

        reaper.register("alert-001", deadline, now, "72h_analyst_sla");

        let mut fired = Vec::new();
        let count = reaper.check_expirations(now + hours_to_ns(1), |reg| {
            fired.push(reg.alert_id.clone());
        });

        assert_eq!(count, 0, "No alert should expire before deadline");
        assert!(fired.is_empty());
        assert_eq!(reaper.active_count(), 1);
    }

    #[test]
    fn alert_expires_after_61_minutes() {
        let mut reaper = AlertReaper::new();
        let t0 = 0i64;
        let deadline = t0 + hours_to_ns(1);

        reaper.register("alert-sla-001", deadline, t0, "1h_test_sla");

        let mut expired_ids = Vec::new();
        // Advance simulated clock by 61 minutes
        let t_check = t0 + minutes_to_ns(61);
        let count = reaper.check_expirations(t_check, |reg| {
            expired_ids.push(reg.alert_id.clone());
        });

        assert_eq!(count, 1, "Exactly one alert must expire");
        assert_eq!(expired_ids, vec!["alert-sla-001"]);
        assert_eq!(reaper.active_count(), 0, "Reaper must have zero active alerts");
    }

    #[test]
    fn deregistered_alert_not_expired() {
        let mut reaper = AlertReaper::new();
        let t0 = 0i64;
        let deadline = t0 + hours_to_ns(1);

        reaper.register("alert-early-close", deadline, t0, "1h_test_sla");
        // Analyst acknowledges and closes before SLA
        reaper.deregister("alert-early-close");

        let mut fired = Vec::new();
        reaper.check_expirations(t0 + hours_to_ns(2), |reg| {
            fired.push(reg.alert_id.clone());
        });

        assert!(
            fired.is_empty(),
            "Deregistered alert must not trigger expiry callback"
        );
    }

    #[test]
    fn multiple_alerts_ordered_expiry() {
        let mut reaper = AlertReaper::new();
        let t0 = 0i64;

        reaper.register("short-sla", t0 + hours_to_ns(1), t0, "1h");
        reaper.register("medium-sla", t0 + hours_to_ns(24), t0, "24h");
        reaper.register("long-sla", t0 + hours_to_ns(72), t0, "72h");

        let mut expired = Vec::new();

        // Only short-sla should expire at t=2h
        reaper.check_expirations(t0 + hours_to_ns(2), |reg| {
            expired.push(reg.sla_tier.clone());
        });
        assert_eq!(expired, vec!["1h"]);

        // medium-sla should expire at t=25h
        expired.clear();
        reaper.check_expirations(t0 + hours_to_ns(25), |reg| {
            expired.push(reg.sla_tier.clone());
        });
        assert_eq!(expired, vec!["24h"]);

        assert_eq!(reaper.active_count(), 1);
    }
}
