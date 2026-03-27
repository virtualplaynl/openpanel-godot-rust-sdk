/// Tracking user used for identify user calls

use serde::Serialize;
use std::collections::HashMap;

/// User object used for identify user calls
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifyUser {
    pub profile_id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub properties: HashMap<String, String>,
}
