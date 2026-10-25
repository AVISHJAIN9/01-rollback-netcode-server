/// Feature 74: Tamper-Evident Alert Lifecycle History Journal
///
/// Each `JournalEntry` is cryptographically linked to its predecessor via
/// SHA-256 chaining:
///
///   H_n = SHA-256( H_{n-1} ‖ seq ‖ timestamp_ns ‖ from_state ‖ to_state
///                          ‖ actor ‖ comment )
///
/// The chain root uses the 32-byte zero vector as the sentinel previous hash.
/// `VerifyIntegrity` re-derives every hash from first principles and compares
/// it to the stored value, detecting any in-place mutation of any field.

use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

use super::alert_state_machine::AlertState;

/// A single immutable entry in the alert lifecycle journal.
#[derive(Debug, Clone)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number (0-based).
    pub seq: u64,
    /// Wall-clock time the event was recorded, nanoseconds since UNIX epoch.
    pub timestamp_ns: i64,
    /// State the alert occupied before this event.
    pub from_state: AlertState,
    /// State the alert moved to.
    pub to_state: AlertState,
    /// Principal (human analyst, automated service, reaper, etc.) causing the event.
    pub actor: String,
    /// Free-text comment or enrichment payload (may be empty).
    pub comment: String,
    /// SHA-256 digest of the previous entry (zero-vector for the first entry).
    pub prev_hash: [u8; 32],
    /// SHA-256 digest over all fields of this entry plus prev_hash.
    pub entry_hash: [u8; 32],
}

impl JournalEntry {
    /// Derive the canonical byte representation used as SHA-256 input.
    ///
    /// Layout (all fields big-endian where applicable):
    ///   prev_hash(32) ‖ seq(8) ‖ timestamp_ns(8) ‖ from_state_u8(1) ‖
    ///   to_state_u8(1) ‖ actor_len(4) ‖ actor_bytes ‖ comment_len(4) ‖ comment_bytes
    fn canonical_bytes(
        prev_hash: &[u8; 32],
        seq: u64,
        timestamp_ns: i64,
        from_state: AlertState,
        to_state: AlertState,
        actor: &str,
        comment: &str,
    ) -> Vec<u8> {
        let actor_bytes = actor.as_bytes();
        let comment_bytes = comment.as_bytes();

        let capacity = 32 + 8 + 8 + 1 + 1 + 4 + actor_bytes.len() + 4 + comment_bytes.len();
        let mut buf = Vec::with_capacity(capacity);

        buf.extend_from_slice(prev_hash);
        buf.extend_from_slice(&seq.to_be_bytes());
        buf.extend_from_slice(&timestamp_ns.to_be_bytes());
        buf.push(alert_state_as_u8(from_state));
        buf.push(alert_state_as_u8(to_state));
        buf.extend_from_slice(&(actor_bytes.len() as u32).to_be_bytes());
        buf.extend_from_slice(actor_bytes);
        buf.extend_from_slice(&(comment_bytes.len() as u32).to_be_bytes());
        buf.extend_from_slice(comment_bytes);

        buf
    }

    /// Recompute the entry hash from scratch and return it.  Used by
    /// `VerifyIntegrity` to detect mutations without trusting `self.entry_hash`.
    pub fn recompute_hash(&self) -> [u8; 32] {
        let canonical = Self::canonical_bytes(
            &self.prev_hash,
            self.seq,
            self.timestamp_ns,
            self.from_state,
            self.to_state,
            &self.actor,
            &self.comment,
        );
        let digest = Sha256::digest(&canonical);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }
}

/// Map `AlertState` to a stable single-byte discriminant.
fn alert_state_as_u8(s: AlertState) -> u8 {
    match s {
        AlertState::New => 0x01,
        AlertState::Acknowledged => 0x02,
        AlertState::InTriage => 0x03,
        AlertState::Resolved => 0x04,
        AlertState::Dismissed => 0x05,
        AlertState::ExpiredArchived => 0x06,
    }
}

/// The full lifecycle journal for a single alert.
///
/// All entries are stored in insertion order.  The journal is append-only:
/// there is no method to remove or mutate existing entries.
#[derive(Debug, Default)]
pub struct LifecycleJournal {
    entries: Vec<JournalEntry>,
}

impl LifecycleJournal {
    /// Create a new empty journal.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Number of entries currently in the journal.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the journal contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append a new lifecycle event.
    ///
    /// The caller supplies the state transition and actor; timestamp and
    /// cryptographic linkage are computed internally.
    ///
    /// Returns a reference to the newly appended entry.
    pub fn append_entry(
        &mut self,
        from_state: AlertState,
        to_state: AlertState,
        actor: &str,
        comment: &str,
    ) -> &JournalEntry {
        let seq = self.entries.len() as u64;

        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);

        let prev_hash: [u8; 32] = if let Some(last) = self.entries.last() {
            last.entry_hash
        } else {
            [0u8; 32]
        };

        let canonical = JournalEntry::canonical_bytes(
            &prev_hash,
            seq,
            timestamp_ns,
            from_state,
            to_state,
            actor,
            comment,
        );
        let digest = Sha256::digest(&canonical);
        let mut entry_hash = [0u8; 32];
        entry_hash.copy_from_slice(&digest);

        self.entries.push(JournalEntry {
            seq,
            timestamp_ns,
            from_state,
            to_state,
            actor: actor.to_owned(),
            comment: comment.to_owned(),
            prev_hash,
            entry_hash,
        });

        self.entries.last().unwrap()
    }

    /// Verify the cryptographic integrity of the entire chain.
    ///
    /// Re-derives every entry's hash from its stored fields and asserts:
    ///   1. `entry.entry_hash == recompute_hash(entry)`
    ///   2. `entry.prev_hash == previous_entry.entry_hash` (for seq ≥ 1)
    ///
    /// Returns `true` only if every check passes.  Returns `false` and prints
    /// a diagnostic on the first violation.
    pub fn verify_integrity(&self) -> bool {
        let mut expected_prev = [0u8; 32];

        for (i, entry) in self.entries.iter().enumerate() {
            // Check linkage
            if entry.prev_hash != expected_prev {
                eprintln!(
                    "Journal integrity failure at seq={}: prev_hash mismatch \
                     (expected {}, got {})",
                    i,
                    hex::encode(expected_prev),
                    hex::encode(entry.prev_hash)
                );
                return false;
            }

            // Recompute and compare
            let recomputed = entry.recompute_hash();
            if recomputed != entry.entry_hash {
                eprintln!(
                    "Journal integrity failure at seq={}: entry_hash mismatch \
                     (stored {}, recomputed {})",
                    i,
                    hex::encode(entry.entry_hash),
                    hex::encode(recomputed)
                );
                return false;
            }

            expected_prev = entry.entry_hash;
        }

        true
    }

    /// Read-only slice of all journal entries in insertion order.
    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    /// Mutable access used only in tests to simulate tamper attacks.
    #[cfg(test)]
    pub fn entries_mut(&mut self) -> &mut Vec<JournalEntry> {
        &mut self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_journal_verifies() {
        let journal = LifecycleJournal::new();
        assert!(journal.verify_integrity());
    }

    #[test]
    fn five_entries_chain_verifies() {
        let mut journal = LifecycleJournal::new();

        let transitions = [
            (AlertState::New, AlertState::Acknowledged, "analyst_a", "ACK"),
            (AlertState::Acknowledged, AlertState::InTriage, "analyst_b", "Triage start"),
            (AlertState::InTriage, AlertState::InTriage, "enrichment_svc", "Added EDR context"),
            (AlertState::InTriage, AlertState::InTriage, "enrichment_svc", "Added threat intel"),
            (AlertState::InTriage, AlertState::Resolved, "analyst_a", "Confirmed TP; host isolated"),
        ];

        for (from, to, actor, comment) in &transitions {
            journal.append_entry(*from, *to, actor, comment);
        }

        assert_eq!(journal.len(), 5);
        assert!(
            journal.verify_integrity(),
            "Journal must verify clean after 5 legitimate appends"
        );
    }

    #[test]
    fn tamper_actor_breaks_chain() {
        let mut journal = LifecycleJournal::new();

        journal.append_entry(AlertState::New, AlertState::Acknowledged, "analyst_a", "");
        journal.append_entry(AlertState::Acknowledged, AlertState::InTriage, "analyst_b", "");
        journal.append_entry(AlertState::InTriage, AlertState::Resolved, "analyst_a", "");

        // Pre-tamper integrity must pass
        assert!(journal.verify_integrity());

        // Tamper: overwrite actor in entry[1]
        journal.entries_mut()[1].actor = "attacker_injected".to_string();

        // Post-tamper integrity must fail
        assert!(
            !journal.verify_integrity(),
            "Tampered journal must fail integrity check"
        );
    }

    #[test]
    fn tamper_prev_hash_breaks_chain() {
        let mut journal = LifecycleJournal::new();

        journal.append_entry(AlertState::New, AlertState::Acknowledged, "svc", "");
        journal.append_entry(AlertState::Acknowledged, AlertState::InTriage, "svc", "");

        // Pre-tamper: must pass
        assert!(journal.verify_integrity());

        // Tamper: flip a byte in entry[1].prev_hash
        journal.entries_mut()[1].prev_hash[0] ^= 0xFF;

        assert!(
            !journal.verify_integrity(),
            "Mutated prev_hash must be detected"
        );
    }

    #[test]
    fn sequence_numbers_are_monotonic() {
        let mut journal = LifecycleJournal::new();
        for i in 0..10 {
            journal.append_entry(AlertState::New, AlertState::Acknowledged, "svc", &format!("step {i}"));
        }
        for (i, entry) in journal.entries().iter().enumerate() {
            assert_eq!(entry.seq, i as u64);
        }
    }
}
