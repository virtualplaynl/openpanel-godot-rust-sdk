//! Group types used for group tracking calls

use serde::Serialize;
use std::collections::HashMap;

/// A group entity to be created or updated via `upsert_group`.
///
/// Groups let you associate users with a shared entity — like a company,
/// workspace, or team — and analyze behaviour at that level.
///
/// # Example
///
/// ```rust
/// use openpanel_sdk::sdk::group::Group;
/// use std::collections::HashMap;
///
/// let group = Group {
///     id: "org_acme".to_string(),
///     group_type: "company".to_string(),
///     name: "Acme Inc".to_string(),
///     properties: HashMap::from([
///         ("plan".to_string(), "enterprise".to_string()),
///     ]),
/// };
/// ```
#[derive(Debug, Serialize)]
pub struct Group {
    /// Unique identifier for the group.
    pub id: String,
    /// Category of the group (e.g. `"company"`, `"workspace"`, `"team"`).
    #[serde(rename = "type")]
    pub group_type: String,
    /// Human-readable display name.
    pub name: String,
    /// Optional custom metadata about the group.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, String>,
}
