use godot::classes::{Os, ProjectSettings};
use godot::prelude::*;

use crate::TrackerError;
use crate::sdk::{Tracker, hashmap_to_dict};
use std::collections::HashMap;

#[derive(GodotClass)]
#[class(init, singleton)]
pub struct Analytics {
    tracker: Option<Tracker>,
    force_in_editor: bool,
    disabled: bool,
    base: Base<Object>,
}

#[godot_api]
impl Analytics {
    #[func]
    pub fn init(&mut self, tree: Gd<SceneTree>, url: String, client_id: String, client_secret: String) {
        self.base_mut().call_deferred("_init_internal", &[Variant::from(tree), Variant::from(url), Variant::from(client_id), Variant::from(client_secret)]);
    }

    #[func]
    fn _init_internal(&mut self, tree: Gd<SceneTree>, url: String, client_id: String, client_secret: String) {
        let mut global_properties = HashMap::new();
        global_properties.insert("os".to_string(), Os::singleton().get_name().to_string());
        global_properties.insert(
            "os-version".to_string(),
            Os::singleton().get_version().to_string(),
        );
        global_properties.insert(
            "version".to_string(),
            ProjectSettings::singleton()
                .get_setting("application/config/version")
                .to_string(),
        );
        global_properties.insert(
            "rust_lib_version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );

        let tracker = Tracker::new(
            tree,
            url,
            client_id,
            client_secret,
        )
        .with_default_headers();
        if tracker.is_err() {
            godot_error!(
                "Failed to set default headers for analytics tracker: {}",
                tracker.err().unwrap()
            );
            return;
        }

        let mut tracker = tracker.unwrap().to_owned();
        tracker.force_in_editor(self.force_in_editor);
        tracker.disable(self.disabled);

        self.tracker = Some(tracker.with_global_properties(global_properties));
        if self.tracker.unwrap().is_disabled() {
            if Os::singleton().has_feature("engine") {
                godot_print!("OpenPanel Analytics are disabled while running in engine\nYou can enable them by calling Analytics.enable() in your code");
            } else {
                godot_print!("OpenPanel Analytics are disabled");
            }
        } else {
            let tracker_clone = self.tracker.unwrap();
            godot::task::spawn(async {
                let (_tracker, success) = Analytics::_init_async(tracker_clone).await;
                if !success {
                    godot_warn!("Failed to initialize analytics");
                }
            });
        }
    }

    async fn _init_async(mut tracker: Tracker) -> (Tracker, bool) {
        let result = tracker.track("app_started".to_string(), None, None).await;
        if let Ok(response) = result {
            if response.result == godot::classes::http_request::Result::SUCCESS
                && response.response_code >= 200
                && response.response_code < 300
            {
                godot_print!(
                    "Successfully tracked app start: {}",
                    response.body.get_string_from_utf8()
                );
                (tracker, true)
            } else {
                godot_error!(
                    "Failed to track app start (HTTP {}): {}",
                    response.response_code,
                    response.body.get_string_from_utf8()
                );
                (tracker, false)
            }
        } else {
            match result.err().unwrap() {
                TrackerError::NotAuthorized => {
                    godot_error!("Analytics tracking failed: Not Authorized")
                }
                TrackerError::TooManyRequests => {
                    godot_error!("Analytics tracking failed: Too Many Requests")
                }
                TrackerError::Internal => godot_error!("Analytics tracking failed: Internal Error"),
                TrackerError::Request => godot_error!("Analytics tracking failed: Request Error"),
                TrackerError::Serializing(error) => {
                    godot_error!("Analytics tracking failed: Serializing Error: {}", error)
                }
                TrackerError::HeaderName => {
                    godot_error!("Analytics tracking failed: Invalid Header Name")
                }
                TrackerError::HeaderValue => {
                    godot_error!("Analytics tracking failed: Invalid Header Value")
                }
                TrackerError::Disabled => {}
                TrackerError::Filtered => {}
            };
            (tracker, false)
        }
    }

    fn _track_event_internal(
        &mut self,
        event: String,
        profile_id: Option<String>,
        properties: Option<VarDictionary>,
        filter: Option<&dyn Fn(HashMap<String, String>) -> bool>,
    ) {
        if self.tracker.is_some() {
            let mut tracker = self.tracker.clone().unwrap();
            if !tracker.clone().filter(properties.clone(), filter) {
                godot_print!("Analytics event '{}' was filtered out", event);
                return;
            }
            godot::task::spawn(async move {
                let result = tracker.track(event, profile_id, properties).await;
                if let Err(err) = result {
                    match err {
                        TrackerError::NotAuthorized => {
                            godot_error!("Analytics tracking failed: Not Authorized");
                        }
                        TrackerError::TooManyRequests => {
                            godot_error!("Analytics tracking failed: Too Many Requests");
                        }
                        TrackerError::Internal => {
                            godot_error!("Analytics tracking failed: Internal Error");
                        }
                        TrackerError::Request => {
                            godot_error!("Analytics tracking failed: Request Error");
                        }
                        TrackerError::Serializing(e) => {
                            godot_error!("Analytics tracking failed: Serializing Error: {}", e);
                        }
                        TrackerError::HeaderName => {
                            godot_error!("Analytics tracking failed: Invalid Header Name");
                        }
                        TrackerError::HeaderValue => {
                            godot_error!("Analytics tracking failed: Invalid Header Value");
                        }
                        TrackerError::Disabled => {}
                        TrackerError::Filtered => {}
                    }
                }
            });
        } else {
            godot_error!("Analytics tracker not initialized");
        }
    }

    #[allow(unused)]
    #[func]
    pub fn force_in_editor(&mut self, force: bool) {
        self.force_in_editor = force;
        if let Some(tracker) = self.tracker.as_mut() { tracker.force_in_editor(force); }
    }

    #[allow(unused)]
    #[func]
    /// Disable sending events to OpenPanel
    pub fn disable(&mut self, disable: bool) {
        self.disabled = disable;
        if let Some(tracker) = self.tracker.as_mut() { tracker.disable(disable); }
    }

    #[allow(unused)]
    #[func]
    pub fn is_disabled(&self) -> bool {
        if let Some(tracker) = &self.tracker {
            tracker.is_disabled()
        } else {
            self.disabled
        }
    }


    #[allow(unused)]
    #[func]
    pub fn track_event(&mut self, event: String, properties: Variant) {
        self._track_event_internal(event, None, if properties != Variant::nil() { Some(properties.to::<VarDictionary>()) } else { None }, None);
    }

    #[allow(unused)]
    pub fn track_event_with_properties(&mut self, event: String, properties: HashMap<String, String>) {
        self._track_event_internal(event, None, Some(hashmap_to_dict(properties)), None);
    }

    #[allow(unused)]
    #[func]
    pub fn track_event_bare(&mut self, event: String) {
        self._track_event_internal(event, None, None, None);
    }

    #[allow(unused)]
    pub fn track_event_with_profile_id_and_properties(&mut self, event: String, profile_id: String, properties: HashMap<String, String>) {
        self._track_event_internal(event, Some(profile_id), Some(hashmap_to_dict(properties)), None);
    }

    #[allow(unused)]
    #[func]
    pub fn track_event_with_profile_id(
        &mut self,
        event: String,
        profile_id: String,
        properties: Variant,
    ) {
        self._track_event_internal(
            event,
            Some(profile_id),
            if properties != Variant::nil() { Some(properties.to::<VarDictionary>()) } else { None },
            None,
        );
    }

    #[allow(unused)]
    pub fn track_event_with_filter(
        &mut self,
        event: String,
        properties: Option<HashMap<String, String>>,
        filter: Option<&dyn Fn(HashMap<String, String>) -> bool>,
    ) {
        self._track_event_internal(event, None, properties.map(|p| hashmap_to_dict(p)), filter);
    }

    #[allow(unused)]
    pub fn track_event_with_profile_id_and_filter(
        &mut self,
        event: String,
        profile_id: Option<String>,
        properties: Option<HashMap<String, String>>,
        filter: Option<&dyn Fn(HashMap<String, String>) -> bool>,
    ) {
        self._track_event_internal(
            event,
            profile_id,
            properties.map(|p| hashmap_to_dict(p)),
            filter,
        );
    }
}
