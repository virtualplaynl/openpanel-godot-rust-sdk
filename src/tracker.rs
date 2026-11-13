/// OpenPanel SDK for tracking events
///
/// # Example
///
/// ```rust
/// use openpanel_sdk::sdk::Tracker;
/// use std::collections::HashMap;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
///     let mut properties = HashMap::new();
///
///     properties.insert("name".to_string(), "rust".to_string());
///
///     tracker.track("test".to_string(), None, Some(properties), None).await?;
///
///     Ok(())
/// }
/// ```
///
/// or apply filter
///
/// ```rust
/// use openpanel_sdk::sdk::Tracker;
/// use std::collections::HashMap;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let filter = |properties: HashMap<String, String>| properties.contains_key("name");
///     let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
///     let mut properties = HashMap::new();
///
///     properties.insert("name".to_string(), "rust".to_string());
///
///     // will return error because properties contain key "name"
///     let result = tracker.track("test".to_string(), None, Some(properties), Some(&filter)).await;
///
///     assert!(result.is_err());
///
///     Ok(())
/// }
/// ```
use crate::{Analytics, TrackerError, TrackerResult, group, user};
use godot::classes::http_client::Method;
use godot::classes::{ConfigFile, Engine, HttpRequest, Json, Os};
use godot::global::Error;
use godot::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Display;

const DEBUG: bool = false;

/// Type of event to track
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
enum TrackType {
    /// Decrement property value on OpenPanel
    Decrement,
    /// Create or update a group entity on OpenPanel
    Group,
    /// Identify property value on OpenPanel
    Identify,
    /// Increment property value on OpenPanel
    Increment,
    /// Link a profile to one or more groups on OpenPanel
    AssignGroup,
    /// Track event on OpenPanel
    #[default]
    Track,
}

impl Display for TrackType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn user_agent() -> String {
    let engine_version = Engine::singleton()
        .get_version_info()
        .get("string")
        .unwrap_or_default();
    let mut os_name = Os::singleton().get_name();
    let os_version = Os::singleton().get_version();
    let cargo_version = env!("CARGO_PKG_VERSION");
    let mut device_type = Os::singleton().get_model_name();
    if os_name.to_lower() == "windows" {
        device_type = ("Windows NT ".to_string() + &os_version.to_string())
            .as_str()
            .into();
    } else if os_name.to_lower() == "macos" {
        device_type = "Macintosh".into();
    } else if os_name.to_lower() == "linux" {
        device_type = ("X11; ".to_string() + &device_type.to_string())
            .as_str()
            .into();
    } else if device_type.to_lower().begins_with("iphone") {
        os_name = "CPU iPhone OS".into();
    } else if device_type.to_lower().begins_with("ipad") {
        os_name = "CPU iPad OS".into();
    }

    format!(
        "OpenPanelRustSDK/{} ({}; {} {}) Godot Engine/{}",
        cargo_version, device_type, os_name, os_version, engine_version
    )
}

pub struct HttpRequestResult {
    pub result: godot::classes::http_request::Result,
    pub response_code: i64,
    pub headers: PackedStringArray,
    pub body: PackedByteArray,
}

pub fn dict_to_hashmap(dict: Dictionary<GString, GString>) -> HashMap<String, String> {
    let mut hashmap = HashMap::new();
    for (key, value) in dict.iter_shared() {
        hashmap.insert(key.to_string(), value.to_string());
    }
    hashmap
}

pub fn hashmap_to_dict(hashmap: HashMap<String, String>) -> Dictionary<GString, GString> {
    let mut dict = Dictionary::new();
    for (key, value) in hashmap.iter() {
        dict.set(&GString::from(key), &GString::from(value));
    }
    dict
}

/// OpenPanel SDK for tracking events
#[derive(GodotClass)]
#[class(base=Node)]
pub struct OpenPanelTracker {
    api_url: String,
    http_client: Gd<HttpRequest>,
    device_id: Option<String>,
    headers: Dictionary<GString, GString>,
    global_props: HashMap<String, String>,
    groups: HashSet<String>,
    store_session_device: bool,
    force_in_editor: bool,
    disabled: bool,
    busy: bool,
    send_queue: VecDeque<serde_json::Value>,
    base: Base<Node>,
}

#[godot_api]
impl INode for OpenPanelTracker {
    fn init(base: Base<Node>) -> Self {
        let api_url = std::env::var("OPENPANEL_API_URL")
            .unwrap_or_else(|_| "https://api.openpanel.dev".to_string());
        let client_id = std::env::var("OPENPANEL_CLIENT_ID").unwrap_or_default();
        let client_secret = std::env::var("OPENPANEL_CLIENT_SECRET").unwrap_or_default();
        Self::_init_node(
            api_url,
            client_id,
            client_secret,
            HashSet::new(),
            true,
            false,
            true,
            base,
        )
    }

    fn enter_tree(&mut self) {
        let http_client = self.http_client.clone();
        let store_session_device = self.store_session_device;
        let force_in_editor = self.force_in_editor;
        let disabled = self.disabled;

        http_client
            .signals()
            .request_completed()
            .connect_other(self, Self::on_request_completed);

        let mut base = self.base_mut();
        base.add_child(&http_client);

        let mut analytics = Analytics::with_tracker(
            base.clone().cast(),
            store_session_device,
            force_in_editor,
            disabled,
        );
        analytics.bind_mut().store();
    }

    fn process(&mut self, _delta: f64) {
        if self.busy {
            return;
        }

        if let Some(payload) = self.send_queue.pop_front() {
            godot_print!(
                "Sending payload: {}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
            self.send_request(payload);
        }
    }

    fn exit_tree(&mut self) {
        self.http_client.queue_free();
        Analytics::clear_tracker(self.base_mut().clone().cast());
    }
}

impl OpenPanelTracker {
    /// Create new tracker instance from string credentials
    fn _init_node(
        api_url: String,
        client_id: String,
        client_secret: String,
        groups: HashSet<String>,
        store_session_device: bool,
        force_in_editor: bool,
        disabled: bool,
        base: Base<Node>,
    ) -> Self {
        let mut config = ConfigFile::new_gd();
        let device_id = if config.load("user://tracker.cfg") == Error::OK {
            let id = config.get_value("tracker", "device_id").to_string();
            Analytics::set_device_id(&id);
            Some(id)
        } else {
            None
        };

        Self {
            api_url,
            http_client: HttpRequest::new_alloc(),
            device_id: device_id.clone(),
            headers: dict! {
                "Content-Type" => "application/json",
                "User-Agent" => user_agent().as_str(),
                "openpanel-client-id" => client_id.as_str(),
                "openpanel-client-secret" => client_secret.as_str(),
            },
            global_props: if let Some(known_id) = device_id {
                HashMap::from([("__deviceId".to_string(), known_id)])
            } else {
                HashMap::new()
            },
            groups: groups,
            store_session_device: store_session_device,
            force_in_editor,
            disabled,
            busy: false,
            send_queue: VecDeque::new(),
            base,
        }
    }

    /// Sets credentials and options for the tracker.
    pub fn set(
        &mut self,
        api_url: String,
        client_id: String,
        client_secret: String,
        store_session_device: bool,
        force_in_editor: bool,
        disabled: bool,
    ) {
        self.api_url = api_url;
        self.headers = dict! {
            "Content-Type" => "application/json",
            "User-Agent" => user_agent().as_str(),
            "openpanel-client-id" => client_id.as_str(),
            "openpanel-client-secret" => client_secret.as_str(),
        };
        self.store_session_device = store_session_device;
        self.force_in_editor = force_in_editor;
        self.disabled = disabled;
    }

    pub fn set_device_id(&mut self, device_id: String) {
        if self.store_session_device {
            self.device_id = Some(device_id.clone());
            Analytics::set_device_id(&device_id);

            let mut config = ConfigFile::new_gd();
            config.set_value("tracker", "device_id", &Variant::from(device_id.clone()));
            let err = config.save("user://tracker.cfg");
            if err != Error::OK {
                godot_error!("Failed to save device ID to config: {:?}", err);
            }

            self.global_props
                .insert("__deviceId".to_string(), device_id);
        }
    }

    pub fn get_device_id(&self) -> Option<String> {
        self.device_id.clone()
    }

    /// Set a custom header for a tracker object.
    /// Use this to set custom headers used for e.g. geo location
    pub fn set_header(&mut self, key: String, value: String) {
        self.headers
            .set(&GString::from(&key), &GString::from(&value));
    }

    /// Set global properties for tracker object. Global properties are added to every
    /// `track` and `identify` event sent.
    pub fn set_global_properties(&mut self, properties: HashMap<String, String>) {
        self.global_props = properties;
    }

    pub fn force_in_editor(&mut self, force: bool) {
        self.force_in_editor = force;
    }

    /// Disable sending events to OpenPanel
    pub fn disable(&mut self, disable: bool) {
        self.disabled = disable;
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled || (Os::singleton().has_feature("editor") && !self.force_in_editor)
    }

    pub fn filter(
        &self,
        properties: Option<Dictionary<GString, GString>>,
        filter: Option<&dyn Fn(HashMap<String, String>) -> bool>,
    ) -> bool {
        let properties_map = properties.map(|p| dict_to_hashmap(p));

        if let Some(filter) = filter {
            if filter(self.create_properties_with_globals(properties_map.clone())) {
                return false;
            }
        }
        true
    }

    /// Track event on OpenPanel
    ///
    /// # Parameters:
    /// - event [String]: The event name
    /// - properties [Option<HashMap<String, String>>]: Additional properties to send with the event
    /// - callback [Option<&dyn Fn(HttpRequestResult) -> bool>]: If provided, the callback fn will
    ///     be called with the result of the request. If the result is true, the event will be considered successful
    pub fn track(
        &mut self,
        event: &str,
        profile_id: Option<String>,
        properties: Option<Dictionary<GString, GString>>,
        groups: Option<Array<GString>>,
    ) {
        let properties_map = properties.map(|p| dict_to_hashmap(p));

        let properties = self.create_properties_with_globals(properties_map);
        // Merge persistent groups with per-event groups
        let merged_groups = {
            match groups {
                Some(event_groups) => {
                    let mut merged = self.groups.clone();
                    merged.extend(event_groups.iter_shared().map(|g| g.to_string()));
                    if merged.is_empty() {
                        None
                    } else {
                        Some(merged)
                    }
                }
                None => {
                    if self.groups.is_empty() {
                        None
                    } else {
                        Some(self.groups.clone())
                    }
                }
            }
        };
        let mut payload = if profile_id.is_some() {
            serde_json::json!({
                "type": TrackType::Track,
                "payload": {
                    "profileId": profile_id,
                    "name": event,
                    "properties": properties
                }
            })
        } else {
            serde_json::json!({
                "type": TrackType::Track,
                "payload": {
                    "name": event,
                    "properties": properties
                }
            })
        };

        if let Some(ref g) = merged_groups {
            payload["payload"]["groups"] = serde_json::json!(g);
        }
        self.queue_request(payload);
    }

    /// Identify user on OpenPanel
    pub fn identify(&mut self, mut user: user::IdentifyUser) {
        user.properties = self.create_properties_with_globals(Some(user.properties));

        let payload = serde_json::json!({
          "type": TrackType::Identify,
          "payload": user
        });

        self.queue_request(payload);
    }

    /// Decrement property value on OpenPanel
    pub fn decrement(&mut self, profile_id: String, property: String, value: i64) {
        let payload = serde_json::json!({
          "type": TrackType::Decrement,
          "payload": {
            "profileId": profile_id,
            "property": property,
            "value": value
          }
        });

        self.queue_request(payload);
    }

    /// Decrement property value on OpenPanel
    pub fn increment(&mut self, profile_id: String, property: String, value: i64) {
        let payload = serde_json::json!({
          "type": TrackType::Increment,
          "payload": {
            "profileId": profile_id,
            "property": property,
            "value": value
          }
        });

        self.queue_request(payload);
    }

    /// Create or update a group entity on OpenPanel.
    ///
    /// Use this to sync dynamic group metadata (plan, seats, MRR, etc.) from
    /// your own data. Call it on login or when group properties change — not on
    /// every request.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use openpanel_sdk::sdk::{group::Group, Tracker};
    /// use std::collections::HashMap;
    ///
    /// #[tokio::main]
    /// async fn main() -> anyhow::Result<()> {
    ///     let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
    ///     let group = Group {
    ///         id: "org_acme".to_string(),
    ///         group_type: "company".to_string(),
    ///         name: "Acme Inc".to_string(),
    ///         properties: HashMap::from([("plan".to_string(), "enterprise".to_string())]),
    ///     };
    ///     tracker.upsert_group(group).await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn upsert_group(&mut self, group: group::Group) {
        let payload = serde_json::json!({
            "type": TrackType::Group,
            "payload": group
        });

        self.queue_request(payload);
    }

    /// Link the current profile to a single group ID.
    ///
    /// Sends an `assign_group` event to OpenPanel and persists the group ID on
    /// the SDK instance so that all subsequent `track()` calls automatically
    /// include it.
    ///
    /// The group does not need to exist yet — events will be tagged with the
    /// group ID and the group can be created later via `upsert_group` or in the
    /// dashboard.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use openpanel_sdk::sdk::Tracker;
    ///
    /// #[tokio::main]
    /// async fn main() -> anyhow::Result<()> {
    ///     let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
    ///     tracker.set_group("user_123".to_string(), "org_acme".to_string()).await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn set_group(&mut self, profile_id: String, group_id: String) {
        // Persist the group ID on the tracker instance
        {
            self.groups.insert(group_id.clone());
        }

        // Send the assign_group event
        let payload = serde_json::json!({
            "type": TrackType::AssignGroup,
            "payload": {
                "profileId": profile_id,
                "groups": [group_id]
            }
        });

        self.queue_request(payload);
    }

    /// Link the current profile to multiple group IDs at once.
    ///
    /// Sends a single `assign_group` event containing all provided group IDs
    /// and persists them on the SDK instance so that all subsequent `track()`
    /// calls automatically include them.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use openpanel_sdk::sdk::Tracker;
    ///
    /// #[tokio::main]
    /// async fn main() -> anyhow::Result<()> {
    ///     let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
    ///     tracker
    ///         .set_groups(
    ///             "user_123".to_string(),
    ///             vec!["org_acme".to_string(), "team_engineering".to_string()],
    ///         )
    ///         .await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn set_groups(&mut self, profile_id: String, group_ids: Vec<String>) {
        // Persist the group IDs on the tracker instance
        {
            self.groups.extend(group_ids.clone().into_iter());
        }

        // Send the assign_group event
        let payload = serde_json::json!({
            "type": TrackType::AssignGroup,
            "payload": {
                "profileId": profile_id,
                "groups": group_ids
            }
        });

        self.queue_request(payload);
    }

    /// Clear all persisted state on the tracker, including groups.
    ///
    /// Always call this on logout to prevent a new user from inheriting the
    /// previous user's groups.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use openpanel_sdk::sdk::Tracker;
    ///
    /// fn handle_logout(tracker: &Tracker) {
    ///     tracker.clear();
    /// }
    /// ```
    pub fn clear(&mut self) {
        self.groups.clear();
    }

    pub fn revenue(
        &mut self,
        profile_id: Option<String>,
        amount: i64,
        properties: Option<Dictionary<GString, GString>>,
    ) {
        let local_props = HashMap::from([("__revenue".to_string(), amount.to_string())]);
        let mut properties =
            self.create_properties_with_globals(properties.map(|p| dict_to_hashmap(p)));

        properties.extend(local_props);

        self.track(
            "revenue",
            profile_id,
            Some(hashmap_to_dict(properties)),
            None,
        );
    }

    pub async fn fetch_device_id(&mut self) -> TrackerResult<String> {
        if self.disabled {
            return Err(TrackerError::Disabled);
        }

        let url = format!("{}/device-id", self.api_url);
        if Os::singleton().has_feature("editor") && !self.force_in_editor {
            return Err(TrackerError::Disabled);
        }
        if Os::singleton().is_debug_build() && DEBUG {
            godot_print!("Fetching device ID from {}", url);
        }

        let err = self
            .http_client
            .request_ex(url.as_str())
            .custom_headers(&PackedStringArray::from_iter(
                self.headers
                    .iter_shared()
                    .map(|(k, v)| GString::from(format!("{}: {}", k, v).as_str())),
            ))
            .done();

        if err != Error::OK {
            godot_error!("Failed to send request: {:?}", err);
            return Err(TrackerError::Request);
        }

        let (result, _response_code, _headers, body) = self
            .http_client
            .signals()
            .request_completed()
            .to_future()
            .await;
        if result != godot::classes::http_request::Result::SUCCESS.ord() as i64 {
            godot_error!("Failed to fetch device ID: {:?}", result);
            return Err(TrackerError::Request);
        }
        let json = serde_json::from_str::<HashMap<String, String>>(body.to_string().as_str())?;
        let id = if !json.contains_key("deviceId") {
            return Ok("".to_string());
        } else {
            json.get("deviceId").unwrap().to_string()
        };

        Ok(id)
    }

    /// Extend given properties with global properties
    fn create_properties_with_globals(
        &self,
        properties: Option<HashMap<String, String>>,
    ) -> HashMap<String, String> {
        if let Some(mut properties) = properties {
            properties.extend(self.global_props.clone());
            properties
        } else {
            self.global_props.clone()
        }
    }

    fn queue_request(&mut self, payload: serde_json::Value) {
        self.send_queue.push_back(payload);
    }

    /// Actually send the request to the API
    fn send_request(&mut self, payload: serde_json::Value) {
        self.busy = true;

        let mut client = self.http_client.clone();

        if self.disabled {
            return;
        }

        if Os::singleton().has_feature("editor") && !self.force_in_editor {
            return;
        }

        if Os::singleton().is_debug_build() && DEBUG {
            godot_print!("Sending request to {}", self.api_url);
            godot_print!(
                "Sending payload:\n{}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
        }

        let api_url = self.api_url.as_str();

        let err = client
            .request_ex(api_url)
            .request_data(&serde_json::to_string(&payload).unwrap_or_default())
            .custom_headers(&PackedStringArray::from_iter(
                self.headers
                    .iter_shared()
                    .map(|(k, v)| GString::from(format!("{}: {}", k, v).as_str())),
            ))
            .method(Method::POST)
            .done();

        if err != Error::OK {
            godot_error!("Failed to send request: {:?}", err);
            self.busy = false;
            return;
        }
    }

    fn on_request_completed(
        &mut self,
        result: i64,
        response_code: i64,
        headers: PackedStringArray,
        body: PackedByteArray,
    ) {
        if result != godot::classes::http_request::Result::SUCCESS.ord() as i64 {
            godot_error!("Failed to send request: {:?}", result);
            return;
        }

        if Os::singleton().is_debug_build() && DEBUG {
            godot_print!("Request completed with response code: {}", response_code);
            godot_print!("Response headers: {:?}", headers);
            godot_print!(
                "Response body:\n{}",
                String::from_utf8_lossy(&body.to_vec())
            );
        }

        self.busy = false;

        let mut json = Json::new_gd();
        json.parse(&body.get_string_from_utf8());
        let json = json.get_data().to::<VarDictionary>();
        let device_id = json.get("deviceId").unwrap_or(Variant::nil());
        if !device_id.is_nil() {
            self.set_device_id(device_id.to_string());
            self.busy = false;
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use reqwest::header::HeaderValue;
//     use serde_json::json;

//     fn get_profile_id() -> Option<String> {
//         Some("rust_123123123".to_string())
//     }

//     #[test]
//     fn can_set_default_headers() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;

//         assert_eq!(
//             tracker.headers.get("Content-Type").unwrap(),
//             "application/json".parse::<HeaderValue>()?
//         );
//         assert_eq!(
//             tracker.headers.get("openpanel-client-id").unwrap(),
//             std::env::var("OPENPANEL_CLIENT_ID")
//                 .unwrap()
//                 .parse::<HeaderValue>()?
//         );
//         assert_eq!(
//             tracker.headers.get("openpanel-client-secret").unwrap(),
//             std::env::var("OPENPANEL_CLIENT_SECRET")
//                 .unwrap()
//                 .parse::<HeaderValue>()?
//         );

//         Ok(())
//     }

//     #[test]
//     fn can_set_custom_header() -> anyhow::Result<()> {
//         let tracker =
//             Tracker::try_new_from_env()?.with_header("test".to_string(), "test".to_string())?;

//         assert_eq!(
//             tracker.headers.get("test").unwrap(),
//             "test".parse::<HeaderValue>()?
//         );

//         Ok(())
//     }

//     #[test]
//     fn can_create_properties_with_globals() -> anyhow::Result<()> {
//         let properties = HashMap::from([("test".to_string(), "test".to_string())]);
//         let tracker = Tracker::try_new_from_env()?.with_global_properties(properties.clone());
//         let properties_with_globals =
//             tracker.create_properties_with_globals(Some(properties.clone()));

//         assert_eq!(tracker.global_props, properties_with_globals);

//         Ok(())
//     }

//     #[test]
//     fn can_set_global_properties() -> anyhow::Result<()> {
//         let properties = HashMap::from([("test".to_string(), "test".to_string())]);
//         let tracker = Tracker::try_new_from_env()?.with_global_properties(properties.clone());

//         assert_eq!(tracker.global_props, properties);

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_send_request() -> anyhow::Result<()> {
//         let payload = json!({
//           "type": TrackType::Track,
//           "payload": {
//             "name": "test_event",
//             "properties": {
//               "name": "rust"
//             }
//           }
//         });

//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
//         let response = tracker.send_request(payload).await?;

//         assert_eq!(response.status(), 200);

//         Ok(())
//     }

//     #[tokio::test]
//     async fn cannot_send_request_if_disabled() -> anyhow::Result<()> {
//         let payload = json!({
//           "type": TrackType::Track,
//           "payload": {
//             "name": "test_event",
//             "properties": {
//               "name": "rust"
//             }
//           }
//         });

//         let tracker = Tracker::try_new_from_env()?
//             .with_default_headers()?
//             .disable();
//         let response = tracker.send_request(payload).await;

//         assert!(response.is_err());

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_track_event() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
//         let mut properties = HashMap::new();

//         properties.insert("name".to_string(), "rust".to_string());

//         let response = tracker
//             .track(
//                 "test_event".to_string(),
//                 get_profile_id(),
//                 Some(properties),
//                 None,
//             )
//             .await?;

//         assert_eq!(response.status(), 200);

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_filter_track_event() -> anyhow::Result<()> {
//         let filter = |properties: HashMap<String, String>| properties.contains_key("name");
//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
//         let mut properties = HashMap::new();

//         properties.insert("name".to_string(), "rust".to_string());

//         let response = tracker
//             .track(
//                 "test_event".to_string(),
//                 get_profile_id(),
//                 Some(properties),
//                 Some(&filter),
//             )
//             .await;

//         assert!(response.is_err());

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_identify_user() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
//         let mut properties = HashMap::new();

//         properties.insert("name".to_string(), "rust".to_string());

//         let user = user::IdentifyUser {
//             profile_id: "test_profile_id".to_string(),
//             email: "rust@test.com".to_string(),
//             first_name: "Rust".to_string(),
//             last_name: "Rust".to_string(),
//             properties,
//         };

//         let response = tracker.identify(user).await?;

//         assert_eq!(response.status(), 200);

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_increment_property() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
//         let response = tracker
//             .increment(
//                 "test_profile_id".to_string(),
//                 "test_property".to_string(),
//                 1,
//             )
//             .await?;

//         assert_eq!(response.status(), 200);

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_decrement_property() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
//         let response = tracker
//             .decrement(
//                 "test_profile_id".to_string(),
//                 "test_property".to_string(),
//                 1,
//             )
//             .await?;

//         assert_eq!(response.status(), 200);

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_track_revenue() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
//         let response = tracker.revenue(get_profile_id(), 100, None).await?;

//         assert_eq!(response.status(), 200);

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_fetch_device_id() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?
//             .with_default_headers()?
//             .with_header("user-agent".to_string(), "some".to_string())?;
//         let id = tracker.fetch_device_id().await?;

//         assert!(!id.is_empty());

//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_set_group_persists_group() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.disable();
//         // Even though the tracker is disabled (send_request returns error),
//         // the group ID should still be persisted.
//         let _ = tracker
//             .set_group("profile_123".to_string(), "org_acme".to_string())
//             .await;
//         let groups = tracker.groups.read().unwrap();
//         assert!(groups.contains(&"org_acme".to_string()));
//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_set_groups_persists_multiple() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.disable();
//         let _ = tracker
//             .set_groups(
//                 "profile_123".to_string(),
//                 vec!["org_acme".to_string(), "team_engineering".to_string()],
//             )
//             .await;
//         let groups = tracker.groups.read().unwrap();
//         assert!(groups.contains(&"org_acme".to_string()));
//         assert!(groups.contains(&"team_engineering".to_string()));
//         Ok(())
//     }

//     #[tokio::test]
//     async fn can_clear_groups() -> anyhow::Result<()> {
//         let tracker = Tracker::try_new_from_env()?.disable();
//         let _ = tracker
//             .set_group("profile_123".to_string(), "org_acme".to_string())
//             .await;
//         tracker.clear();
//         let groups = tracker.groups.read().unwrap();
//         assert!(groups.is_empty());
//         Ok(())
//     }
// }
