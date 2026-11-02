//! `a0-chat` — conversation features over the verified Agent Zero wire APIs.
//!
//! - [`markdown`]: bounded markdown-to-native-blocks rendering (no raw HTML
//!   from agent output, pathological input truncated).
//! - [`composer`]: outbound send flows (direct/multipart/queued) with the
//!   a0-core delivery FSM — explicit transitions only, no auto-resend.

pub mod composer;
pub mod error;
pub mod markdown;

pub use composer::{AttachmentPlan, ComposerService, OutboundMessage};
pub use error::{ChatError, Result};
pub use markdown::{render_blocks, Block};
