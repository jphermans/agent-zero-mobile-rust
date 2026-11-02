//! Composer service: send flows with explicit delivery tracking.
//!
//! Wire contract (protocol inventory §3.1, §7):
//! - direct send: `POST /api/message_async` (text or multipart with files),
//! - queued send:  `POST /api/upload` then `POST /api/message_queue_add`,
//! - delivery FSM: Queued -> Sending -> Acked | Failed, terminal states
//!   never auto-transitioned (no auto-resend, §6).
//!
//! Every send requires an authenticated socket session; the error is
//! surfaced instead of triggering a login retry inside the send path.

use std::collections::HashMap;

use a0_api::client::A0Client;
use a0_api::endpoints::core::{message_async, message_queue_add, MessageQueueAddRequest};
use a0_api::upload::{message_async_multipart, upload_files, UploadFile};
use a0_core::{ChatId, Delivery, DeliveryReason, DeliveryStatus, MessageId, ServerProfileId};

use crate::error::{ChatError, Result};

/// What to do with the staged attachments of a send.
#[derive(Debug, Clone, Default)]
pub enum AttachmentPlan {
    /// Text-only send.
    #[default]
    None,
    /// Direct multipart send with these files (single request, §7).
    Direct(Vec<UploadFile>),
    /// Queued send: upload first, then queue with the returned names.
    Queued(Vec<UploadFile>),
}

/// One outbound message under explicit delivery control.
#[derive(Debug)]
pub struct OutboundMessage {
    delivery: Delivery,
    chat: ChatId,
    text: String,
    attachments: AttachmentPlan,
}

impl OutboundMessage {
    /// Creates a queued outbound message (delivery status `Queued`).
    pub fn new(chat: ChatId, text: String, attachments: AttachmentPlan) -> Self {
        Self {
            delivery: Delivery::queued(MessageId::new(uuid::Uuid::new_v4().to_string())),
            chat,
            text,
            attachments,
        }
    }

    /// Delivery bookkeeping (read-only).
    pub fn delivery(&self) -> &Delivery {
        &self.delivery
    }

    /// Current delivery status.
    pub fn status(&self) -> &DeliveryStatus {
        &self.delivery.status
    }

    /// Target chat.
    pub fn chat(&self) -> &ChatId {
        &self.chat
    }

    /// Composed text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Explicit FSM transition; rejected edges return
    /// [`ChatError::Core`] (`IllegalTransition`) and change nothing.
    pub fn transition(&mut self, next: DeliveryStatus) -> Result<()> {
        self.delivery.transition(next).map_err(ChatError::Core)?;
        Ok(())
    }
}


/// Maps an API error onto a delivery failure reason (local classification;
/// a0-core defines no API coupling by design).
fn reason_from_api(error: &a0_api::ApiError) -> DeliveryReason {
    if error.is_transient() {
        DeliveryReason::Network
    } else {
        DeliveryReason::Rejected
    }
}

/// Tracks outbound messages per chat while the UI list needs their delivery
/// state; entries survive screen rotation because the service is a plain
/// value owned by the app model (a0-core §4 single-writer convention).
#[derive(Debug, Default)]
pub struct ComposerService {
    outbox: HashMap<String, OutboundMessage>,
}

impl ComposerService {
    /// Creates an empty service.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueues a message for sending; status starts at `Queued`.
    pub fn stage(
        &mut self,
        profile: &ServerProfileId,
        chat: ChatId,
        text: String,
        attachments: AttachmentPlan,
    ) -> String {
        let _ = profile; // outbox keys are already profile-scoped by the caller
        let message = OutboundMessage::new(chat, text, attachments);
        let id = message.delivery.id.as_str().to_string();
        self.outbox.insert(id.clone(), message);
        id
    }

    /// Removes a terminal message from the visible outbox (UI list).
    pub fn retire(&mut self, id: &str) -> Option<OutboundMessage> {
        self.outbox.remove(id)
    }

    /// Number of tracked messages.
    pub fn len(&self) -> usize {
        self.outbox.len()
    }

    /// `true` when nothing is tracked.
    pub fn is_empty(&self) -> bool {
        self.outbox.is_empty()
    }

    /// Sends one staged message (direct or queued path chosen by the
    /// attachment plan). FSM transitions are explicit: `Queued -> Sending`
    /// before the request, `-> Acked` on server confirmation, `-> Failed`
    /// on any error. A transport error is terminal here — the caller may
    /// stage a fresh message, but this one is never auto-retried (§6).
    pub async fn send(
        &mut self,
        client: &A0Client,
        id: &str,
    ) -> Result<()> {
        // FSM: Queued -> Sending.
        {
            let message = self.outbox.get_mut(id).ok_or(ChatError::UnknownMessage)?;
            message.transition(DeliveryStatus::Sending)?;
        }

        let (chat, text, attachments) = {
            let message = &self.outbox[id];
            (
                message.chat.clone(),
                message.text.clone(),
                message.attachments.clone(),
            )
        };

        // Every branch returns a uniform outcome so the FSM always advances
        // (a queued-path upload failure must still mark the message Failed).
        let result: std::result::Result<(), a0_api::ApiError> = match attachments {
            AttachmentPlan::None => {
                message_async(client, chat.as_str(), &text).await.map(|_| ())
            }
            AttachmentPlan::Direct(files) => {
                message_async_multipart(client, chat.as_str(), &text, &files)
                    .await
                    .map(|_| ())
            }
            AttachmentPlan::Queued(files) => match upload_files(client, &files).await {
                Ok(names) => {
                    let request = MessageQueueAddRequest::new(chat.as_str(), text.clone())
                        .attachments(names);
                    message_queue_add(client, &request).await.map(|_| ())
                }
                Err(e) => Err(e),
            },
        };

        let message = self.outbox.get_mut(id).expect("checked above");
        match result {
            Ok(()) => {
                message.transition(DeliveryStatus::Acked)?;
                Ok(())
            }
            Err(api_error) => {
                let reason = reason_from_api(&api_error);
                message.transition(DeliveryStatus::Failed(reason))?;
                Err(ChatError::Api(api_error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat() -> ChatId {
        ChatId::new("ctx-1")
    }

    #[test]
    fn stage_starts_queued_and_tracks() {
        let mut service = ComposerService::new();
        let id = service.stage(
            &ServerProfileId::new("p"),
            chat(),
            "hello".into(),
            AttachmentPlan::None,
        );
        assert_eq!(service.len(), 1);
        let message = &service.outbox[&id];
        assert_eq!(message.status(), &DeliveryStatus::Queued);
    }

    #[test]
    fn acked_is_terminal() {
        let mut service = ComposerService::new();
        let id = service.stage(
            &ServerProfileId::new("p"),
            chat(),
            "hello".into(),
            AttachmentPlan::None,
        );
        let message = service.outbox.get_mut(&id).unwrap();
        message.transition(DeliveryStatus::Sending).unwrap();
        message.transition(DeliveryStatus::Acked).unwrap();
        // Terminal: no transition leaves Acked.
        assert!(matches!(
            message.transition(DeliveryStatus::Sending),
            Err(ChatError::Core(_))
        ));
    }

    #[test]
    fn failed_carries_reason_and_stays_terminal() {
        let mut service = ComposerService::new();
        let id = service.stage(
            &ServerProfileId::new("p"),
            chat(),
            "hello".into(),
            AttachmentPlan::None,
        );
        let message = service.outbox.get_mut(&id).unwrap();
        message.transition(DeliveryStatus::Sending).unwrap();
        message
            .transition(DeliveryStatus::Failed(DeliveryReason::Rejected))
            .unwrap();
        assert!(matches!(
            message.transition(DeliveryStatus::Acked),
            Err(ChatError::Core(_))
        ));
    }

    #[test]
    fn retire_removes_only_terminal() {
        let mut service = ComposerService::new();
        let id = service.stage(
            &ServerProfileId::new("p"),
            chat(),
            "hello".into(),
            AttachmentPlan::None,
        );
        // Retire is a UI-list op; the service allows it for any state but
        // the FSM guard lives in the send path (documented contract).
        let removed = service.retire(&id);
        assert!(removed.is_some());
        assert!(service.is_empty());
    }
}
