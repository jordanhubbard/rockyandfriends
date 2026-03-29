// sc_types.rs — SquirrelChat shared type definitions
// Single source of truth for all SC data structures.
// Both squirrelchat.rs (Natasha) and future component modules (Bullwinkle) import from here.

use serde::{Deserialize, Serialize};

// ─── Message ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScMessage {
    pub id: Option<String>,
    pub from: Option<String>,
    pub text: Option<String>,
    pub channel: Option<String>,
    pub ts: Option<String>,
    pub mentions: Option<Vec<String>>,
    /// ID of the parent message if this is a thread reply
    pub parent_id: Option<String>,
    /// Number of replies in this message's thread (top-level messages only)
    pub thread_count: Option<u32>,
    /// Emoji reactions on this message
    pub reactions: Option<Vec<ScReaction>>,
    /// Whether this message has been edited
    pub edited: Option<bool>,
    /// Attached files
    pub files: Option<Vec<ScAttachment>>,
}

// ─── Reaction ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScReaction {
    pub emoji: String,
    pub count: u32,
    /// Whether the current user has reacted with this emoji
    pub by_me: Option<bool>,
}

// ─── Attachment ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScAttachment {
    pub id: String,
    pub filename: String,
    pub size: Option<u64>,
    pub content_type: Option<String>,
    pub url: Option<String>,
}

// ─── Channel ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScChannel {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// "public" | "private" | "dm"
    pub kind: Option<String>,
    pub unread_count: Option<u32>,
    pub members: Option<Vec<String>>,
}

// ─── User / Agent ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScUser {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    /// "human" | "agent"
    pub kind: Option<String>,
    pub online: Option<bool>,
    pub status: Option<String>,
}

/// Legacy alias — keep until all callers migrate to ScUser
pub type ScAgent = ScUser;

// ─── Project ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScProject {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub tags: Option<Vec<String>>,
}

// ─── File (project file listing) ─────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScFile {
    pub name: String,
    pub size: Option<u64>,
    pub created_at: Option<String>,
}

// ─── Auth / Identity ─────────────────────────────────────────────────────────

/// The current user's identity, fetched from /sc/api/me or stored in localStorage
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ScIdentity {
    pub id: String,
    pub name: String,
    pub token: Option<String>,
}

// ─── Fallbacks (used before dynamic data loads) ───────────────────────────────

pub const DEFAULT_CHANNELS: &[(&str, &str)] = &[
    ("general", "General"),
    ("agents", "Agents"),
    ("ops", "Ops"),
    ("random", "Random"),
];

pub const FALLBACK_AGENT_NAMES: &[&str] =
    &["natasha", "rocky", "bullwinkle", "sparky", "boris"];
