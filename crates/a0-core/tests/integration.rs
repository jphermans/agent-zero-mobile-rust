//! Integration tests: reducer + delivery interplay, driven by the section-8
//! fixtures of the phase-1 protocol inventory.
//!
//! These tests exercise the two cooperating halves of the shared core:
//! the synchronization reducer (protocol inventory sec 5.4) and the delivery
//! state machine for outbound commands (sec 6: mutations are never
//! auto-retried).

use a0_core::{
    ApplyReport, ChatId, Delivery, DeliveryReason, DeliveryStatus, Effect, HandshakeAck, MessageId,
    Snapshot, StatePush, StateRequest, SyncReducer,
};
use serde_json::{json, Value};

/// Protocol inventory sec 8.1 full snapshot (`Fixtures/full.json`), verbatim.
const FULL_SNAPSHOT: &str = r#"{
  "deselect_chat": false,
  "context": "synthetic-chat",
  "contexts": [{"id": "synthetic-chat", "name": "Synthetic fixture", "running": false}],
  "tasks": [],
  "logs": [
    {"no": 0, "id": "synthetic-user", "type": "user", "heading": "", "content": "Synthetic request", "kvps": {}, "timestamp": 0, "agentno": 0},
    {"no": 1, "id": "synthetic-response", "type": "response", "heading": "", "content": "Working", "kvps": {}, "timestamp": 1, "agentno": 0}
  ],
  "log_guid": "synthetic-log",
  "log_version": 2,
  "log_progress": "Working",
  "log_progress_active": true,
  "paused": false,
  "notifications": [{"id": "synthetic-notice", "no": 0, "message": "Synthetic notice", "read": false}],
  "notifications_guid": "synthetic-notifications",
  "notifications_version": 1
}"#;

/// Protocol inventory sec 8.6 valid handshake ack, verbatim.
const HANDSHAKE_ACK_VALID: &str = r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"runtime","seq_base":1}}]}"#;

/// Protocol inventory sec 8.7 application error inside a successful ack,
/// verbatim; the ack decodes but must fail validation.
const HANDSHAKE_ACK_APP_ERROR: &str = r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"code":"INVALID_REQUEST"}}]}"#;

/// Builds the sec 8.2 delta snapshot: identical key set with the documented
/// differences applied (null collections, single upserted log, new versions).
fn delta_snapshot() -> Snapshot {
    let mut value: Value = serde_json::from_str(FULL_SNAPSHOT).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.insert("contexts".into(), Value::Null);
    obj.insert("tasks".into(), Value::Null);
    obj.insert(
        "logs".into(),
        json!([{"no": 1, "id": "synthetic-response", "type": "response", "heading": "", "content": "Completed synthetic response", "kvps": {}, "timestamp": 2, "agentno": 0}]),
    );
    obj.insert("log_version".into(), json!(3));
    obj.insert("log_progress".into(), json!(""));
    obj.insert("log_progress_active".into(), json!(false));
    obj["notifications"][0]["read"] = json!(true);
    obj.insert("notifications_version".into(), json!(2));
    serde_json::from_value(value).unwrap()
}

/// Builds the fixture-based full snapshot.
fn full_snapshot() -> Snapshot {
    serde_json::from_str(FULL_SNAPSHOT).unwrap()
}

/// Arms a reducer with the sec 8.6 ack and commits the sec 8.1 fixture.
fn bootstrapped_reducer() -> SyncReducer {
    let ack: HandshakeAck = serde_json::from_str(HANDSHAKE_ACK_VALID).unwrap();
    let ack_data = ack.validated("request-1").expect("fixture 8.6 must validate");
    assert_eq!(ack_data.runtime_epoch, "runtime");
    assert_eq!(ack_data.seq_base, 1);

    let mut reducer = SyncReducer::new();
    reducer.begin_handshake(ack_data.runtime_epoch, ack_data.seq_base);
    let (report, effects) = reducer.apply_snapshot("runtime", full_snapshot());
    assert_eq!(report, ApplyReport::Applied { full: true });
    assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
    assert!(!reducer.needs_full_sync());
    reducer
}

#[test]
fn handshake_ack_fixture_drives_full_sync_bootstrap() {
    let reducer = bootstrapped_reducer();
    assert_eq!(reducer.epoch(), Some("runtime"));
    assert_eq!(reducer.sequence(), 1);

    let state = reducer.current_snapshot().expect("full snapshot committed");
    assert_eq!(state.context, "synthetic-chat");
    assert_eq!(state.logs.len(), 2);
    assert_eq!(state.log_version, 2);
    assert_eq!(state.notifications_version, 1);

    // Derived request uses the committed versions while no full sync is
    // pending (sec 5.4, SyncReducer.swift:38-41).
    let request = reducer.state_request("UTC");
    assert_eq!(request.log_from, 2);
    assert_eq!(request.notifications_from, 1);
    assert_eq!(request.context, Some(ChatId::new("synthetic-chat")));
}

#[test]
fn app_error_ack_fixture_is_rejected_before_any_handshake() {
    let ack: HandshakeAck = serde_json::from_str(HANDSHAKE_ACK_APP_ERROR).unwrap();
    assert!(ack.validated("request-1").is_err());

    // A rejected ack must not arm the reducer: pushes stay buffered.
    let mut reducer = SyncReducer::new();
    let (report, effects) = reducer
        .apply_push(StatePush::new("runtime", 2, delta_snapshot()))
        .unwrap();
    assert!(matches!(report, ApplyReport::Buffered { pending: 1 }));
    assert_eq!(effects, vec![Effect::Noop]);
}

#[test]
fn command_delivery_round_trip_then_delta_push_applies() {
    let mut reducer = bootstrapped_reducer();

    // Outbound command follows the documented FSM edges only (sec 6).
    let mut command = Delivery::queued(MessageId::new("m-1"));
    command.transition(DeliveryStatus::Sending).unwrap();
    command.transition(DeliveryStatus::Acked).unwrap();
    assert_eq!(command.status, DeliveryStatus::Acked);
    assert!(command.transition(DeliveryStatus::Sending).is_err(), "no auto-resend edge");

    // The next in-sequence delta (sec 8.2 fixture) applies on top.
    let (report, effects) = reducer
        .apply_push(StatePush::new("runtime", 2, delta_snapshot()))
        .unwrap();
    assert_eq!(report, ApplyReport::Applied { full: false });
    assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
    assert_eq!(reducer.sequence(), 2);

    let state = reducer.current_snapshot().unwrap();
    assert_eq!(state.logs.len(), 2, "upsert by no must not duplicate");
    assert_eq!(
        state.logs[1].content.as_deref(),
        Some("Completed synthetic response")
    );
    assert_eq!(state.log_version, 3);
    assert!(state.notifications[0].read);

    // From-offsets advance for the next request.
    let request = reducer.state_request("UTC");
    assert_eq!(request.log_from, 3);
    assert_eq!(request.notifications_from, 2);
}

#[test]
fn failed_delivery_does_not_resend_and_state_stays_consistent() {
    let mut reducer = bootstrapped_reducer();
    let mut command = Delivery::queued(MessageId::new("m-2"));
    command.transition(DeliveryStatus::Sending).unwrap();
    command
        .transition(DeliveryStatus::Failed(DeliveryReason::Timeout))
        .unwrap();

    // Terminal: the caller decides on a retry (new intent), the FSM itself
    // has no resend edge (sec 6).
    for next in [
        DeliveryStatus::Queued,
        DeliveryStatus::Sending,
        DeliveryStatus::Acked,
        DeliveryStatus::Failed(DeliveryReason::Network),
    ] {
        assert!(command.transition(next).is_err());
    }

    // Meanwhile the stream keeps flowing: an in-sequence push applies.
    let (report, _) = reducer
        .apply_push(StatePush::new("runtime", 2, delta_snapshot()))
        .unwrap();
    assert_eq!(report, ApplyReport::Applied { full: false });
}

#[test]
fn stale_epoch_after_recovery_requires_resync_and_request_resets_offsets() {
    let mut reducer = bootstrapped_reducer();

    // Sync interruption: epoch cleared, sequence 0, full sync required,
    // display kept (sec 6, SyncReducer.swift:35-37).
    reducer.invalidate_for_recovery();
    assert!(reducer.current_snapshot().is_some());
    let request = reducer.state_request("UTC");
    assert_eq!(request.log_from, 0, "offsets collapse to 0 while recovering");
    assert_eq!(request.notifications_from, 0);

    // Pushes buffer until the next handshake arms a new epoch.
    let (report, effects) = reducer
        .apply_push(StatePush::new("runtime", 3, delta_snapshot()))
        .unwrap();
    assert!(matches!(report, ApplyReport::Buffered { pending: 1 }));
    assert_eq!(effects, vec![Effect::Noop]);

    // New handshake with a fresh epoch; the buffered push carries the OLD
    // epoch and must be rejected as stale, flagging a resync (sec 5.4).
    reducer.begin_handshake("runtime-2", 0);
    let pending = reducer.take_pending_pushes();
    assert_eq!(pending.len(), 1);
    let stale = StatePush::new(pending[0].runtime_epoch.clone(), pending[0].seq, pending[0].snapshot.clone());
    let (report, effects) = reducer.apply_push(stale).unwrap();
    assert_eq!(report, ApplyReport::Rejected { reason: a0_core::PushRejectReason::StaleEpoch });
    assert_eq!(effects, vec![Effect::RequestResync]);
    assert!(reducer.needs_full_sync());

    // A full snapshot for the new epoch recovers the reducer.
    let (report, effects) = reducer.apply_snapshot("runtime-2", full_snapshot());
    assert_eq!(report, ApplyReport::Applied { full: true });
    assert_eq!(effects, vec![Effect::EmitSnapshotChanged]);
    assert!(!reducer.needs_full_sync());
}

#[test]
fn pre_handshake_overflow_fails_the_attempt_without_corruption() {
    let mut reducer = SyncReducer::new();
    for seq in 1..=32u64 {
        let (report, effects) = reducer
            .apply_push(StatePush::new("runtime", seq, full_snapshot()))
            .unwrap();
        assert!(matches!(report, ApplyReport::Buffered { .. }));
        assert_eq!(effects, vec![Effect::Noop]);
    }
    assert_eq!(reducer.pending_count(), 32);

    // Push 33 fails the whole attempt with BufferOverflow
    // (failure stage payload-or-handshake, Realtime.swift:53-56).
    assert!(reducer
        .apply_push(StatePush::new("runtime", 33, full_snapshot()))
        .is_err());
    assert_eq!(reducer.pending_count(), 32, "overflowing push must not partially land");
}

#[test]
fn serialized_request_matches_the_sec_8_4_poll_fixture_shape() {
    // Disarmed reducer: full sync pending -> offsets 0, context null.
    let reducer = SyncReducer::new();
    let request: StateRequest = reducer.state_request("UTC");
    let encoded: Value = serde_json::to_value(&request).unwrap();
    let expected: Value =
        serde_json::from_str(r#"{"context": null, "log_from": 0, "notifications_from": 0, "timezone": "UTC", "collections_delta": true}"#).unwrap();
    assert_eq!(encoded, expected);
    assert!(encoded.as_object().unwrap().contains_key("context"));
    assert!(!encoded.as_object().unwrap().contains_key("forceFull"));
}
