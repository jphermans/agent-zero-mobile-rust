//! The state-synchronization reducer, transcribed from the phase-1 protocol
//! inventory sec 5.4 (`Sources/A0Core/SyncReducer.swift`).
//!
//! The reducer is pure domain logic: every mutating method takes `&mut self`,
//! performs no I/O, and returns an [`ApplyReport`] paired with a list of
//! [`Effect`]s in the `(State, Action) -> (State, Vec<Effect>)` style.
//!
//! Documented rules implemented here (all cited):
//! - `begin_handshake(epoch, seq_base)` arms the reducer: `epoch = e`,
//!   `sequence = seq_base`, `needs_full_sync = true`
//!   (`SyncReducer.swift:31-33`).
//! - A push applies **only** when its `runtime_epoch` matches and
//!   `seq == sequence + 1`; on success `sequence = push.seq`; any other
//!   outcome sets `needs_full_sync` (full resync required)
//!   (`SyncReducer.swift:42-50`). Duplicates (`seq <= sequence`) are dropped
//!   without touching state and flag the resync exactly like gaps.
//! - Snapshot application (`SyncReducer.swift:51-84`): a stale generation is
//!   ignored; `snapshot.context` must equal the selected context unless
//!   `deselect_chat`; incremental snapshots are rejected when a full sync is
//!   pending, when `log_guid`/`notifications_guid` changed, or when versions
//!   regressed; a full snapshot carries non-null `contexts` **and** `tasks`;
//!   logs upsert by `no` (incremental) and sort ascending; notifications
//!   upsert by `id`; `deselect_chat` clears the selection and logs; metadata
//!   commits only on success.
//! - `StateRequest` derivation: from-offsets are `0` while a full sync is
//!   pending, else the collection versions (`SyncReducer.swift:38-41`).
//! - Pre-handshake pushes are buffered in arrival order; the buffer follows
//!   the cited literal guard (`count < 32`, `Realtime.swift:53-56`, sec 9 table:
//!   "32 pushes"). TODO(doc-gap): sec 4 prose says "at most 31" while the cited
//!   guard and sec 9 both describe a 32-entry buffer that rejects the next push;
//!   the literal cited guard is implemented here.

use crate::ids::ChatId;
use crate::protocol::{Snapshot, StatePush, StatePushData, StateRequest};
use crate::CoreError;

/// Side effects requested by the reducer; the host layer executes them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// Nothing to do.
    Noop,
    /// The committed snapshot changed; persist/broadcast it.
    EmitSnapshotChanged,
    /// A full resync is required (documented `needsFullSync` outcome,
    /// inventory sec 5.4).
    RequestResync,
}

/// Diagnostic reason why a state push did not apply
/// (inventory sec 5.4, `SyncReducer.swift:42-50`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PushRejectReason {
    /// Push `runtime_epoch` differs from the armed epoch.
    StaleEpoch,
    /// Push `seq` is greater than `sequence + 1` (gap).
    OutOfOrder,
    /// Push `seq <= sequence` (already applied or older).
    Duplicate,
}

/// Diagnostic reason why a snapshot did not apply
/// (inventory sec 5.4, `SyncReducer.swift:51-84`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SnapshotRejectReason {
    /// The snapshot belongs to a different generation; ignored entirely.
    StaleGeneration,
    /// `snapshot.context` differs from the selected chat.
    ContextMismatch,
    /// An incremental snapshot arrived while a full sync is required.
    RequiresFullSync,
    /// A collection GUID changed (server restart of a collection).
    GuidReset,
    /// A collection version regressed.
    VersionRegression,
}

/// Outcome of an [`SyncReducer::apply_push`] or
/// [`SyncReducer::apply_snapshot`] call.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ApplyReport {
    /// The snapshot was committed (`full` mirrors [`Snapshot::is_full`]).
    Applied {
        /// Whether the applied snapshot was a full snapshot.
        full: bool,
    },
    /// The push was buffered pre-handshake.
    Buffered {
        /// Buffer occupancy after the push.
        pending: usize,
    },
    /// The snapshot was ignored without any state change
    /// (stale generation, `SyncReducer.swift:51-84`).
    Ignored {
        /// Why the snapshot was ignored.
        reason: SnapshotRejectReason,
    },
    /// The push failed the epoch/sequence gate; state is untouched and a
    /// resync is flagged (`SyncReducer.swift:42-50`).
    Rejected {
        /// Why the push was rejected.
        reason: PushRejectReason,
    },
    /// The snapshot failed a delta/full validation rule; state is untouched,
    /// `needs_full_sync` is set (`SyncReducer.swift:51-84`).
    ResyncRequired {
        /// Why a full resync became required.
        reason: SnapshotRejectReason,
    },
}

/// Capacity of the pre-handshake push buffer
/// (inventory sec 4/sec 9; literal guard `count < 32`, `Realtime.swift:53-56`).
pub const PENDING_BUFFER_CAPACITY: usize = 32;

/// Inserts or replaces `entry` in `logs` by its `no` key.
fn upsert_log_by_no(logs: &mut Vec<crate::protocol::LogEntry>, entry: crate::protocol::LogEntry) {
    if let Some(slot) = logs.iter_mut().find(|existing| existing.no == entry.no) {
        *slot = entry;
    } else {
        logs.push(entry);
    }
}

/// Inserts or replaces `entry` in `notifications` by its `id` key
/// (inventory sec 5.4, `SyncReducer.swift:72-79`).
fn upsert_notification_by_id(
    notifications: &mut Vec<crate::protocol::NotificationEntry>,
    entry: crate::protocol::NotificationEntry,
) {
    if let Some(slot) = notifications
        .iter_mut()
        .find(|existing| existing.id == entry.id)
    {
        *slot = entry;
    } else {
        notifications.push(entry);
    }
}

/// Applies state pushes and snapshots in (epoch, sequence) order.
///
/// See the module docs for the cited rule set. The reducer keeps the last
/// committed (merged) [`Snapshot`] as its display state; deltas retain the
/// stored collections (inventory sec 8.2: "collections retained").
#[derive(Debug, Clone)]
pub struct SyncReducer {
    epoch: Option<String>,
    sequence: u64,
    needs_full_sync: bool,
    selected_context: Option<ChatId>,
    state: Option<Snapshot>,
    pending: Vec<StatePushData>,
}

impl Default for SyncReducer {
    fn default() -> Self {
        Self {
            epoch: None,
            sequence: 0,
            needs_full_sync: true,
            selected_context: None,
            state: None,
            pending: Vec::new(),
        }
    }
}

impl SyncReducer {
    /// Creates a disarmed reducer that buffers pushes and requires a full
    /// sync.
    pub fn new() -> Self {
        Self::default()
    }

    /// Arms the reducer with a validated handshake epoch and sequence base
    /// (inventory sec 5.3 + sec 5.4, `SyncReducer.swift:31-33`):
    /// a full sync is required after arming.
    pub fn begin_handshake(&mut self, epoch: impl Into<String>, seq_base: u64) {
        self.epoch = Some(epoch.into());
        self.sequence = seq_base;
        self.needs_full_sync = true;
    }

    /// Invalidates state for recovery (inventory sec 6,
    /// `SyncReducer.swift:35-37`, coordinator calls `App/SpikeModel.swift:491,539,631`):
    /// epoch cleared, sequence `0`, full sync required; the display state is
    /// kept. Subsequent pushes buffer again until the next handshake.
    pub fn invalidate_for_recovery(&mut self) {
        self.epoch = None;
        self.sequence = 0;
        self.needs_full_sync = true;
    }

    /// Drains the pre-handshake buffer in arrival order for replay after a
    /// valid ack (inventory sec 4, `Realtime.swift:100-102`).
    pub fn take_pending_pushes(&mut self) -> Vec<StatePushData> {
        std::mem::take(&mut self.pending)
    }

    /// Number of pushes currently buffered pre-handshake.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// The armed epoch, if any.
    pub fn epoch(&self) -> Option<&str> {
        self.epoch.as_deref()
    }

    /// The current sequence number (last applied push `seq`, or the
    /// handshake sequence base).
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Whether a full snapshot is required before deltas can apply
    /// (documented `needsFullSync`).
    pub fn needs_full_sync(&self) -> bool {
        self.needs_full_sync
    }

    /// The last committed merged snapshot, if any snapshot has been applied.
    pub fn current_snapshot(&self) -> Option<&Snapshot> {
        self.state.as_ref()
    }

    /// Derives the [`StateRequest`] for the current bookkeeping
    /// (inventory sec 5.4, `SyncReducer.swift:38-41`): from-offsets collapse to
    /// `0` while a full sync is pending. `collections_delta` uses the client
    /// default `true` (inventory sec 10).
    pub fn state_request(&self, timezone: impl Into<String>) -> StateRequest {
        let (log_version, notifications_version) = match &self.state {
            Some(state) => (state.log_version, state.notifications_version),
            None => (0, 0),
        };
        StateRequest::from_versions(
            self.selected_context.clone(),
            self.needs_full_sync,
            log_version,
            notifications_version,
            timezone,
            true,
        )
    }

    /// Applies a fetched snapshot for the given connection generation
    /// (inventory sec 5.4, `SyncReducer.swift:51-84`). The generation is the
    /// caller's connection/attempt identity; the wire [`Snapshot`] itself
    /// carries no epoch, so the transport must supply it. A generation that
    /// differs from the armed epoch is ignored entirely (stale attempt).
    ///
    /// This method never fails: documented snapshot rejections surface as
    /// [`ApplyReport::ResyncRequired`], not as [`CoreError`].
    pub fn apply_snapshot(
        &mut self,
        generation: &str,
        snapshot: Snapshot,
    ) -> (ApplyReport, Vec<Effect>) {
        self.apply_snapshot_rules(generation, snapshot)
    }

    /// Shared snapshot application rules
    /// (inventory sec 5.4, `SyncReducer.swift:51-84`).
    fn apply_snapshot_rules(
        &mut self,
        generation: &str,
        mut incoming: Snapshot,
    ) -> (ApplyReport, Vec<Effect>) {
        // Stale generation => ignore entirely, no state change.
        let armed_matches = match &self.epoch {
            Some(armed) => armed == generation,
            None => false,
        };
        if !armed_matches {
            return (
                ApplyReport::Ignored {
                    reason: SnapshotRejectReason::StaleGeneration,
                },
                vec![Effect::Noop],
            );
        }

        // Context gate: snapshot.context must equal the selected chat unless
        // deselect_chat (inventory sec 5.4). No selection yet => the gate is
        // vacuously satisfied and the first commit establishes the selection.
        if !incoming.deselect_chat {
            if let Some(selected) = &self.selected_context {
                if *selected != ChatId::from(incoming.context.clone()) {
                    self.needs_full_sync = true;
                    return (
                        ApplyReport::ResyncRequired {
                            reason: SnapshotRejectReason::ContextMismatch,
                        },
                        vec![Effect::RequestResync],
                    );
                }
            }
        }

        let full = incoming.is_full();
        if !full {
            // Incremental guards, in documented order.
            if let Some(state) = &self.state {
                if self.needs_full_sync {
                    self.needs_full_sync = true;
                    return (
                        ApplyReport::ResyncRequired {
                            reason: SnapshotRejectReason::RequiresFullSync,
                        },
                        vec![Effect::RequestResync],
                    );
                }
                if state.log_guid != incoming.log_guid
                    || state.notifications_guid != incoming.notifications_guid
                {
                    self.needs_full_sync = true;
                    return (
                        ApplyReport::ResyncRequired {
                            reason: SnapshotRejectReason::GuidReset,
                        },
                        vec![Effect::RequestResync],
                    );
                }
                if incoming.log_version < state.log_version
                    || incoming.notifications_version < state.notifications_version
                {
                    self.needs_full_sync = true;
                    return (
                        ApplyReport::ResyncRequired {
                            reason: SnapshotRejectReason::VersionRegression,
                        },
                        vec![Effect::RequestResync],
                    );
                }
            } else {
                // No committed state yet: a delta cannot bootstrap.
                self.needs_full_sync = true;
                return (
                    ApplyReport::ResyncRequired {
                        reason: SnapshotRejectReason::RequiresFullSync,
                    },
                    vec![Effect::RequestResync],
                );
            }
        }

        // Take the previous merged state for collection retention/merge.
        let previous = self.state.take();
        let (prev_contexts, prev_tasks, prev_logs, prev_notifications) = match previous {
            Some(state) => (state.contexts, state.tasks, state.logs, state.notifications),
            None => (None, None, Vec::new(), Vec::new()),
        };

        let deselect = incoming.deselect_chat;
        if deselect {
            // deselect_chat clears the selection and the logs
            // (inventory sec 5.4, `SyncReducer.swift:62`).
            self.selected_context = None;
            incoming.logs = Vec::new();
        } else {
            self.selected_context = Some(ChatId::from(incoming.context.clone()));
        }

        if full {
            // Full snapshot: the incoming payload is authoritative. The
            // collections (contexts/tasks, non-null by is_full), the logs and
            // the notifications collection are taken verbatim from the
            // snapshot; merging with previous state would resurrect entries
            // the server no longer reports (e.g. after a GUID reset,
            // inventory sec 5.4 / sec 6).
        } else {
            // Delta: retain stored collections (inventory sec 8.2), upsert logs
            // by `no` then sort ascending (`SyncReducer.swift:65-70`).
            incoming.contexts = prev_contexts;
            incoming.tasks = prev_tasks;
            if deselect {
                incoming.logs = Vec::new();
            } else {
                let mut logs = prev_logs;
                for entry in incoming.logs.drain(..) {
                    upsert_log_by_no(&mut logs, entry);
                }
                logs.sort_by_key(|entry| entry.no);
                incoming.logs = logs;
            }
            let mut notifications = prev_notifications;
            for entry in incoming.notifications.drain(..) {
                upsert_notification_by_id(&mut notifications, entry);
            }
            incoming.notifications = notifications;
        }

        // Commit metadata only on success; a successful full snapshot clears
        // the full-sync requirement (inventory sec 5.4, sec 6).
        self.needs_full_sync = false;
        self.state = Some(incoming);
        (
            ApplyReport::Applied { full },
            vec![Effect::EmitSnapshotChanged],
        )
    }

    /// Applies a socket `state_push` (inventory sec 5.4, `SyncReducer.swift:42-50`).
    ///
    /// Disarmed reducer: the push is buffered (arrival order) until the
    /// buffer is full, in which case the whole attempt fails with
    /// [`CoreError::BufferOverflow`] (failure stage `payload-or-handshake`,
    /// `Realtime.swift:53-56`).
    ///
    /// Armed reducer: the push applies only at `seq == sequence + 1` with a
    /// matching `runtime_epoch`; any other outcome sets `needs_full_sync` and
    /// yields [`Effect::RequestResync`] while leaving state untouched.
    pub fn apply_push(&mut self, push: StatePush) -> Result<(ApplyReport, Vec<Effect>), CoreError> {
        let StatePushData {
            runtime_epoch,
            seq,
            snapshot,
        } = push.data;

        let Some(armed) = self.epoch.clone() else {
            if self.pending.len() >= PENDING_BUFFER_CAPACITY {
                return Err(CoreError::BufferOverflow(format!(
                    "pre-handshake buffer holds {} pushes; refusing further pushes",
                    self.pending.len()
                )));
            }
            self.pending.push(StatePushData {
                runtime_epoch,
                seq,
                snapshot,
            });
            return Ok((
                ApplyReport::Buffered {
                    pending: self.pending.len(),
                },
                vec![Effect::Noop],
            ));
        };

        if runtime_epoch != armed {
            self.needs_full_sync = true;
            return Ok((
                ApplyReport::Rejected {
                    reason: PushRejectReason::StaleEpoch,
                },
                vec![Effect::RequestResync],
            ));
        }

        match self.sequence.checked_add(1) {
            Some(expected) if seq == expected => {}
            _ => {
                let reason = if seq <= self.sequence {
                    PushRejectReason::Duplicate
                } else {
                    PushRejectReason::OutOfOrder
                };
                self.needs_full_sync = true;
                return Ok((
                    ApplyReport::Rejected { reason },
                    vec![Effect::RequestResync],
                ));
            }
        }

        // Sequence gate passed: advance the sequence, then apply the push's
        // snapshot with the push's (matching) epoch as its generation.
        self.sequence = seq;
        let (report, effects) = self.apply_snapshot_rules(&runtime_epoch, snapshot);
        Ok((report, effects))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::fixtures;
    use serde_json::Value;

    fn full_snapshot() -> Snapshot {
        serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap()
    }

    /// sec 8.2 delta fixture (prose-defined; identical key set, documented
    /// differences applied).
    fn delta_snapshot() -> Snapshot {
        let mut value: Value = serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.insert("contexts".into(), Value::Null);
        obj.insert("tasks".into(), Value::Null);
        obj.insert(
            "logs".into(),
            serde_json::json!([{"no": 1, "id": "synthetic-response", "type": "response", "heading": "", "content": "Completed synthetic response", "kvps": {}, "timestamp": 2, "agentno": 0}]),
        );
        obj.insert("log_version".into(), serde_json::json!(3));
        obj.insert("log_progress".into(), serde_json::json!(""));
        obj.insert("log_progress_active".into(), serde_json::json!(false));
        obj["notifications"][0]["read"] = serde_json::json!(true);
        obj.insert("notifications_version".into(), serde_json::json!(2));
        serde_json::from_value(value).unwrap()
    }

    fn push(epoch: &str, seq: u64, snapshot: Snapshot) -> StatePush {
        StatePush {
            data: StatePushData {
                runtime_epoch: epoch.to_string(),
                seq,
                snapshot,
            },
        }
    }

    fn armed_reducer() -> SyncReducer {
        let mut reducer = SyncReducer::new();
        reducer.begin_handshake("synthetic-runtime", 1);
        let (report, effects) = reducer.apply_snapshot("synthetic-runtime", full_snapshot());
        assert_eq!(report, ApplyReport::Applied { full: true });
        assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
        reducer
    }

    #[test]
    fn fresh_full_sync_commits_state_and_clears_needs_full_sync() {
        let mut reducer = SyncReducer::new();
        assert!(reducer.needs_full_sync());
        assert!(reducer.current_snapshot().is_none());
        reducer.begin_handshake("runtime", 0);
        assert_eq!(reducer.epoch(), Some("runtime"));
        assert_eq!(reducer.sequence(), 0);
        assert!(reducer.needs_full_sync());

        let (report, effects) = reducer.apply_snapshot("runtime", full_snapshot());
        assert_eq!(report, ApplyReport::Applied { full: true });
        assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
        assert!(!reducer.needs_full_sync());

        let state = reducer.current_snapshot().unwrap();
        assert_eq!(state.context, "synthetic-chat");
        assert_eq!(state.contexts.as_ref().unwrap().len(), 1);
        assert_eq!(state.tasks.as_ref().unwrap().len(), 0);
        assert_eq!(state.log_version, 2);

        // Derived request uses the committed versions (no full sync pending).
        let request = reducer.state_request("UTC");
        assert_eq!(request.log_from, 2);
        assert_eq!(request.notifications_from, 1);
        assert_eq!(request.context, Some(ChatId::new("synthetic-chat")));
    }

    #[test]
    fn delta_pushes_apply_in_sequence_order_and_merge_logs() {
        let mut reducer = armed_reducer();

        let (report, effects) = reducer
            .apply_push(push("synthetic-runtime", 2, delta_snapshot()))
            .unwrap();
        assert_eq!(report, ApplyReport::Applied { full: false });
        assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
        assert_eq!(reducer.sequence(), 2);
        let state = reducer.current_snapshot().unwrap();
        // Collections retained from the full snapshot (sec 8.2).
        assert!(state.contexts.is_some() && state.tasks.is_some());
        assert_eq!(state.logs.len(), 2);
        assert_eq!(
            state.logs[1].content.as_deref(),
            Some("Completed synthetic response")
        );
        assert_eq!(state.log_version, 3);
        assert!(state.notifications[0].read);

        // A later delta touching log no:0 keeps logs sorted ascending by no.
        let mut touch: Value = serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap();
        let obj = touch.as_object_mut().unwrap();
        obj.insert("contexts".into(), Value::Null);
        obj.insert("tasks".into(), Value::Null);
        obj.insert("logs".into(), serde_json::json!([{"no": 0, "id": "synthetic-user-2", "type": "user", "heading": "", "content": "Edited request", "kvps": {}, "timestamp": 9, "agentno": 0}]));
        obj.insert("log_version".into(), serde_json::json!(4));
        obj.insert("log_progress".into(), serde_json::json!(""));
        obj.insert("log_progress_active".into(), serde_json::json!(false));
        obj["notifications"] = serde_json::json!([]);
        obj.insert("notifications_version".into(), serde_json::json!(3));
        let touch: Snapshot = serde_json::from_value(touch).unwrap();

        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 3, touch))
            .unwrap();
        assert_eq!(report, ApplyReport::Applied { full: false });
        let state = reducer.current_snapshot().unwrap();
        let nos: Vec<i64> = state.logs.iter().map(|entry| entry.no).collect();
        assert_eq!(nos, vec![0, 1], "logs must stay sorted ascending by no");
        assert_eq!(state.logs[0].content.as_deref(), Some("Edited request"));
        assert_eq!(reducer.sequence(), 3);
    }

    #[test]
    fn duplicate_push_is_dropped_and_flags_resync() {
        let mut reducer = armed_reducer();
        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 2, delta_snapshot()))
            .unwrap();
        assert_eq!(report, ApplyReport::Applied { full: false });
        let before = reducer.current_snapshot().cloned().unwrap();

        // Re-delivery of seq 2: dropped (state untouched) but, per the strict
        // documented rule (SyncReducer.swift:42-50: applies ONLY at
        // seq == sequence + 1), it also flags needsFullSync.
        let (report, effects) = reducer
            .apply_push(push("synthetic-runtime", 2, delta_snapshot()))
            .unwrap();
        assert_eq!(
            report,
            ApplyReport::Rejected {
                reason: PushRejectReason::Duplicate
            }
        );
        assert_eq!(effects, vec![Effect::RequestResync]);
        assert!(reducer.needs_full_sync());
        assert_eq!(
            reducer.current_snapshot(),
            Some(&before),
            "duplicate must not mutate state"
        );
        assert_eq!(reducer.sequence(), 2, "sequence unchanged on rejection");
    }

    #[test]
    fn stale_epoch_push_is_rejected_and_flags_resync() {
        let mut reducer = armed_reducer();
        let (report, effects) = reducer
            .apply_push(push("other-runtime", 2, delta_snapshot()))
            .unwrap();
        assert_eq!(
            report,
            ApplyReport::Rejected {
                reason: PushRejectReason::StaleEpoch
            }
        );
        assert_eq!(effects, vec![Effect::RequestResync]);
        assert!(reducer.needs_full_sync());
    }

    #[test]
    fn sequence_gap_is_rejected_as_out_of_order() {
        let mut reducer = armed_reducer();
        let (report, effects) = reducer
            .apply_push(push("synthetic-runtime", 5, delta_snapshot()))
            .unwrap();
        assert_eq!(
            report,
            ApplyReport::Rejected {
                reason: PushRejectReason::OutOfOrder
            }
        );
        assert_eq!(effects, vec![Effect::RequestResync]);
        assert!(reducer.needs_full_sync());
        assert_eq!(reducer.sequence(), 1);
    }

    #[test]
    fn pre_handshake_pushes_buffer_up_to_32_then_overflow() {
        let mut reducer = SyncReducer::new();
        for seq in 1..=32u64 {
            let (report, effects) = reducer
                .apply_push(push("synthetic-runtime", seq, full_snapshot()))
                .unwrap();
            assert_eq!(
                report,
                ApplyReport::Buffered {
                    pending: seq as usize
                }
            );
            assert_eq!(effects, vec![Effect::Noop]);
        }
        assert_eq!(reducer.pending_count(), PENDING_BUFFER_CAPACITY);

        let overflow = reducer.apply_push(push("synthetic-runtime", 33, full_snapshot()));
        assert!(matches!(overflow, Err(CoreError::BufferOverflow(_))));
        assert_eq!(reducer.pending_count(), PENDING_BUFFER_CAPACITY);
    }

    #[test]
    fn buffered_pushes_replay_in_order_after_handshake() {
        let mut reducer = SyncReducer::new();
        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 2, delta_snapshot()))
            .unwrap();
        assert!(matches!(report, ApplyReport::Buffered { .. }));

        // Handshake arms at seq_base 1; the buffered seq-2 push then applies.
        reducer.begin_handshake("synthetic-runtime", 1);
        assert!(reducer.needs_full_sync());
        let pending = reducer.take_pending_pushes();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].seq, 2);
        assert_eq!(reducer.pending_count(), 0);

        // A full snapshot must land first (needs_full_sync armed by the
        // handshake); then the replayed delta applies.
        let (report, _) = reducer.apply_snapshot("synthetic-runtime", full_snapshot());
        assert_eq!(report, ApplyReport::Applied { full: true });
        let (report, effects) = reducer
            .apply_push(push("synthetic-runtime", 2, delta_snapshot()))
            .unwrap();
        assert_eq!(report, ApplyReport::Applied { full: false });
        assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
        assert_eq!(reducer.sequence(), 2);
    }

    #[test]
    fn guid_reset_and_version_regression_force_full_resync() {
        let mut reducer = armed_reducer();

        // GUID reset (server restart of the log collection).
        let mut reset_json: Value = serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap();
        let obj = reset_json.as_object_mut().unwrap();
        obj.insert("contexts".into(), Value::Null);
        obj.insert("tasks".into(), Value::Null);
        obj.insert("logs".into(), serde_json::json!([]));
        obj.insert("log_guid".into(), serde_json::json!("restarted-log"));
        obj.insert("log_version".into(), serde_json::json!(1));
        obj["notifications"] = serde_json::json!([]);
        obj.insert("notifications_version".into(), serde_json::json!(2));
        let reset: Snapshot = serde_json::from_value(reset_json).unwrap();

        let (report, effects) = reducer
            .apply_push(push("synthetic-runtime", 2, reset))
            .unwrap();
        assert_eq!(
            report,
            ApplyReport::ResyncRequired {
                reason: SnapshotRejectReason::GuidReset
            }
        );
        assert_eq!(effects, vec![Effect::RequestResync]);
        assert!(reducer.needs_full_sync());

        // Any further delta is rejected until a full snapshot lands.
        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 3, delta_snapshot()))
            .unwrap();
        assert_eq!(
            report,
            ApplyReport::ResyncRequired {
                reason: SnapshotRejectReason::RequiresFullSync
            }
        );

        // While the gate is armed, EVERY non-full push collapses to
        // RequiresFullSync (SyncReducer.swift:54) — so re-land a full
        // snapshot first. Snapshots do not touch `sequence`, but every
        // gate-passing push advances it BEFORE application (reducer.rs:472):
        // the seq-2 GuidReset and seq-3 RequiresFullSync pushes left
        // sequence at 3, so the regression push rides seq 4.
        let (report, _) = reducer.apply_snapshot("synthetic-runtime", full_snapshot());
        assert_eq!(report, ApplyReport::Applied { full: true });

        // Version regression on a synced reducer is likewise rejected.
        let mut regression = delta_snapshot();
        regression.log_guid = "synthetic-log".into();
        regression.notifications_guid = "synthetic-notifications".into();
        regression.notifications_version = 0;
        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 4, regression))
            .unwrap();
        assert_eq!(
            report,
            ApplyReport::ResyncRequired {
                reason: SnapshotRejectReason::VersionRegression
            }
        );

        // Full snapshot recovers the reducer and clears the flag.
        let (report, effects) = reducer.apply_snapshot("synthetic-runtime", full_snapshot());
        assert_eq!(report, ApplyReport::Applied { full: true });
        assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
        assert!(!reducer.needs_full_sync());
    }

    #[test]
    fn stale_generation_snapshot_is_ignored_without_state_change() {
        let mut reducer = armed_reducer();
        let before = reducer.current_snapshot().cloned().unwrap();
        let (report, effects) = reducer.apply_snapshot("old-attempt", full_snapshot());
        assert_eq!(
            report,
            ApplyReport::Ignored {
                reason: SnapshotRejectReason::StaleGeneration
            }
        );
        assert_eq!(effects, vec![Effect::Noop]);
        assert_eq!(reducer.current_snapshot(), Some(&before));
        assert!(!reducer.needs_full_sync());
    }

    #[test]
    fn context_mismatch_rejects_and_flags_resync() {
        let mut reducer = armed_reducer();
        let mut other = full_snapshot();
        other.context = "other-chat".into();
        let (report, effects) = reducer.apply_snapshot("synthetic-runtime", other);
        assert_eq!(
            report,
            ApplyReport::ResyncRequired {
                reason: SnapshotRejectReason::ContextMismatch
            }
        );
        assert_eq!(effects, vec![Effect::RequestResync]);
        assert!(reducer.needs_full_sync());
    }

    #[test]
    fn deselect_chat_clears_selection_and_logs() {
        let mut reducer = armed_reducer();
        let mut deselect = delta_snapshot();
        deselect.deselect_chat = true;
        let (report, effects) = reducer
            .apply_push(push("synthetic-runtime", 2, deselect))
            .unwrap();
        assert_eq!(report, ApplyReport::Applied { full: false });
        assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);

        let state = reducer.current_snapshot().unwrap();
        assert!(state.logs.is_empty(), "deselect must clear logs");
        let request = reducer.state_request("UTC");
        assert_eq!(request.context, None, "selection must be cleared");
    }

    #[test]
    fn invalidate_for_recovery_keeps_display_but_requires_full_sync() {
        let mut reducer = armed_reducer();
        reducer.invalidate_for_recovery();
        assert_eq!(reducer.epoch(), None);
        assert_eq!(reducer.sequence(), 0);
        assert!(reducer.needs_full_sync());
        assert!(
            reducer.current_snapshot().is_some(),
            "display state must be kept"
        );

        // Derived request resets from-offsets while recovering.
        let request = reducer.state_request("UTC");
        assert_eq!(request.log_from, 0);
        assert_eq!(request.notifications_from, 0);

        // Pushes buffer again until the next handshake.
        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 9, delta_snapshot()))
            .unwrap();
        assert!(matches!(report, ApplyReport::Buffered { .. }));
    }

    #[test]
    fn notifications_upsert_by_id_without_duplicates() {
        let mut reducer = armed_reducer();
        let mut delta = delta_snapshot();
        delta
            .notifications
            .push(crate::protocol::NotificationEntry {
                id: "second-notice".into(),
                no: 1,
                message: "Second notice".into(),
                read: false,
                extra: Default::default(),
            });
        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 2, delta))
            .unwrap();
        assert_eq!(report, ApplyReport::Applied { full: false });
        let state = reducer.current_snapshot().unwrap();
        assert_eq!(state.notifications.len(), 2);

        // Re-delivering the same id updates in place instead of duplicating.
        let mut update = delta_snapshot();
        update.notifications[0].message = "Synthetic notice (edited)".into();
        let (report, _) = reducer
            .apply_push(push("synthetic-runtime", 3, update))
            .unwrap();
        assert_eq!(report, ApplyReport::Applied { full: false });
        let state = reducer.current_snapshot().unwrap();
        assert_eq!(state.notifications.len(), 2);
        assert_eq!(state.notifications[0].message, "Synthetic notice (edited)");
    }
}
