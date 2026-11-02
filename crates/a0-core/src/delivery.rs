//! Delivery state machine for outbound protocol commands.
//!
//! Commands (sends, controls, plugin/project/model calls) are **never**
//! auto-retried; callers persist intent before invoking (protocol inventory
//! sec 6, `Projects.swift:169`, `Plugins.swift:149`, `ModelPresets.swift:185`,
//! `Attachments.swift:70`). The state machine therefore has exactly three
//! edges and no resend path:
//!
//! ```text
//! Queued -> Sending -> Acked
//!                    -> Failed(reason)
//! ```

use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Why a delivery ended in [`DeliveryStatus::Failed`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeliveryReason {
    /// The caller cancelled the queued/sending item.
    Cancelled,
    /// Transport-level failure (connection, DNS, socket).
    Network,
    /// Request or ack timeout (inventory sec 9: 20 s request / 30 s resource).
    Timeout,
    /// The server rejected the command (documented response validation,
    /// inventory sec 3).
    Rejected,
    /// Caller-driven retry budget exhausted; there is no automatic retry
    /// (inventory sec 6: mutations are never auto-retried).
    RetryExhausted,
}

impl fmt::Display for DeliveryReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Cancelled => "cancelled",
            Self::Network => "network",
            Self::Timeout => "timeout",
            Self::Rejected => "rejected",
            Self::RetryExhausted => "retry_exhausted",
        };
        f.write_str(name)
    }
}

/// Delivery lifecycle status.
///
/// `Acked` and `Failed` are terminal: no transition leaves them, which
/// structurally forbids auto-resend (inventory sec 6).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeliveryStatus {
    /// Stored locally, not yet on the wire.
    Queued,
    /// Request in flight.
    Sending,
    /// Server accepted the command (validated response per inventory sec 3).
    Acked,
    /// Terminal failure with its reason.
    Failed(DeliveryReason),
}

impl fmt::Display for DeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queued => f.write_str("queued"),
            Self::Sending => f.write_str("sending"),
            Self::Acked => f.write_str("acked"),
            Self::Failed(reason) => write!(f, "failed({reason})"),
        }
    }
}

impl DeliveryStatus {
    /// Returns `true` when moving from this status to `next` is a documented
    /// legal edge. Explicit transitions only: `Queued -> Sending`,
    /// `Sending -> Acked`, `Sending -> Failed(any reason)`.
    pub fn can_transition_to(&self, next: &DeliveryStatus) -> bool {
        matches!(
            (self, next),
            (DeliveryStatus::Queued, DeliveryStatus::Sending)
                | (DeliveryStatus::Sending, DeliveryStatus::Acked)
                | (DeliveryStatus::Sending, DeliveryStatus::Failed(_))
        )
    }

    /// Validates and returns the next status, or
    /// [`CoreError::IllegalTransition`] for any edge outside
    /// [`DeliveryStatus::can_transition_to`].
    pub fn transition_to(&self, next: DeliveryStatus) -> Result<DeliveryStatus, CoreError> {
        if self.can_transition_to(&next) {
            Ok(next)
        } else {
            Err(CoreError::IllegalTransition {
                from: self.to_string(),
                to: next.to_string(),
            })
        }
    }
}

/// A tracked outbound command with its current [`DeliveryStatus`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Delivery {
    /// Client-generated command id (`message_id` / `item_id`, inventory sec 3.1).
    pub id: crate::ids::MessageId,
    /// Current lifecycle status.
    pub status: DeliveryStatus,
}

impl Delivery {
    /// Creates a new delivery in the [`DeliveryStatus::Queued`] state.
    pub fn queued(id: crate::ids::MessageId) -> Self {
        Self {
            id,
            status: DeliveryStatus::Queued,
        }
    }

    /// Applies an explicit transition to `next`, updating the status in
    /// place or returning [`CoreError::IllegalTransition`].
    pub fn transition(&mut self, next: DeliveryStatus) -> Result<(), CoreError> {
        self.status = self.status.transition_to(next)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_edges_are_exactly_the_three_documented_ones() {
        assert!(DeliveryStatus::Queued.can_transition_to(&DeliveryStatus::Sending));
        assert!(DeliveryStatus::Sending.can_transition_to(&DeliveryStatus::Acked));
        for reason in [
            DeliveryReason::Cancelled,
            DeliveryReason::Network,
            DeliveryReason::Timeout,
            DeliveryReason::Rejected,
            DeliveryReason::RetryExhausted,
        ] {
            assert!(DeliveryStatus::Sending.can_transition_to(&DeliveryStatus::Failed(reason)));
        }
    }

    #[test]
    fn every_other_edge_is_illegal() {
        let queued_fail = DeliveryStatus::Queued.transition_to(DeliveryStatus::Failed(DeliveryReason::Network));
        assert!(matches!(queued_fail, Err(CoreError::IllegalTransition { .. })));
        assert!(DeliveryStatus::Queued.transition_to(DeliveryStatus::Acked).is_err());
        assert!(DeliveryStatus::Queued.transition_to(DeliveryStatus::Queued).is_err());
        assert!(DeliveryStatus::Sending.transition_to(DeliveryStatus::Queued).is_err());
        assert!(DeliveryStatus::Sending.transition_to(DeliveryStatus::Sending).is_err());

        // Terminal states: no outgoing edges at all.
        assert!(DeliveryStatus::Acked.transition_to(DeliveryStatus::Sending).is_err());
        assert!(DeliveryStatus::Acked.transition_to(DeliveryStatus::Acked).is_err());
        assert!(DeliveryStatus::Acked
            .transition_to(DeliveryStatus::Failed(DeliveryReason::Network))
            .is_err());
        let failed = DeliveryStatus::Failed(DeliveryReason::Timeout);
        assert!(failed.transition_to(DeliveryStatus::Sending).is_err());
        assert!(failed.transition_to(DeliveryStatus::Acked).is_err());
        assert!(failed
            .transition_to(DeliveryStatus::Failed(DeliveryReason::Network))
            .is_err());
    }

    #[test]
    fn no_auto_resend_transition_exists_from_failed_or_acked() {
        // The resend edge (Acked/Failed -> Sending) must not exist.
        let failed = DeliveryStatus::Failed(DeliveryReason::RetryExhausted);
        assert!(!failed.can_transition_to(&DeliveryStatus::Sending));
        assert!(!DeliveryStatus::Acked.can_transition_to(&DeliveryStatus::Sending));
    }

    #[test]
    fn delivery_item_tracks_legal_lifecycle() {
        let mut delivery = Delivery::queued(crate::ids::MessageId::new("m-1"));
        assert_eq!(delivery.status, DeliveryStatus::Queued);
        delivery.transition(DeliveryStatus::Sending).unwrap();
        delivery.transition(DeliveryStatus::Acked).unwrap();
        assert_eq!(delivery.status, DeliveryStatus::Acked);
        assert!(delivery.transition(DeliveryStatus::Sending).is_err());
    }

    #[test]
    fn failed_transition_carries_the_reason() {
        let mut delivery = Delivery::queued(crate::ids::MessageId::new("m-2"));
        delivery.transition(DeliveryStatus::Sending).unwrap();
        delivery
            .transition(DeliveryStatus::Failed(DeliveryReason::Timeout))
            .unwrap();
        assert_eq!(delivery.status, DeliveryStatus::Failed(DeliveryReason::Timeout));
        assert_eq!(delivery.status.to_string(), "failed(timeout)");
    }

    #[test]
    fn statuses_and_reasons_serialize_round_trip() {
        let statuses = vec![
            DeliveryStatus::Queued,
            DeliveryStatus::Sending,
            DeliveryStatus::Acked,
            DeliveryStatus::Failed(DeliveryReason::Cancelled),
            DeliveryStatus::Failed(DeliveryReason::Network),
            DeliveryStatus::Failed(DeliveryReason::Timeout),
            DeliveryStatus::Failed(DeliveryReason::Rejected),
            DeliveryStatus::Failed(DeliveryReason::RetryExhausted),
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let back: DeliveryStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status, "round trip failed for {json}");
        }
        let delivery = Delivery::queued(crate::ids::MessageId::new("m-3"));
        let json = serde_json::to_string(&delivery).unwrap();
        let back: Delivery = serde_json::from_str(&json).unwrap();
        assert_eq!(back, delivery);
    }

    #[test]
    fn illegal_transition_error_reports_both_sides() {
        let err = DeliveryStatus::Acked
            .transition_to(DeliveryStatus::Sending)
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("acked"), "message must name the source: {message}");
        assert!(message.contains("sending"), "message must name the target: {message}");
    }
}
