use godot::classes::{EditorPlugin, IEditorPlugin, Os, ProjectSettings};
use godot::prelude::*;
use godot::tools::{get_autoload_by_name, try_get_autoload_by_name};

use crate::TrackerError;
use crate::tracker::{OpenPanelTracker, hashmap_to_dict};
use std::collections::HashMap;

#[derive(GodotClass)]
#[class(tool, init, base=EditorPlugin)]
pub struct AnalyticsPlugin {
    base: Base<EditorPlugin>,
}

#[godot_api]
impl IEditorPlugin for AnalyticsPlugin {
    fn enter_tree(&mut self) {
        if try_get_autoload_by_name::<OpenPanelTracker>("OpenPanel").is_err() {
            self.base_mut()
                .add_autoload_singleton("OpenPanel", "res://addons/OpenPanel/OpenPanel.tscn");
        }
    }

    fn exit_tree(&mut self) {
        self.base_mut().remove_autoload_singleton("OpenPanel");
    }
}

#[derive(GodotClass)]
#[class(singleton)]
pub struct Analytics {
    tracker: Option<Gd<OpenPanelTracker>>,
    store_session_device: bool,
    force_in_editor: bool,
    disabled: bool,
    base: Base<Object>,
}

#[godot_api]
impl IObject for Analytics {
    fn init(base: Base<Object>) -> Self {
        Self {
            tracker: None,
            store_session_device: true,
            force_in_editor: false,
            disabled: false,
            base,
        }
    }
}

#[godot_api]
impl Analytics {
    pub fn tracker(&mut self) -> Gd<OpenPanelTracker> {
        if self.tracker.is_none() {
            let tracker = get_autoload_by_name::<OpenPanelTracker>("OpenPanel");
            self.tracker.replace(tracker.clone());
        }

        self.tracker.clone().unwrap()
    }

    #[func]
    pub fn connect(&mut self, url: String, client_id: String, client_secret: String) {
        self.base_mut().call_deferred(
            "_connect_internal",
            &[
                Variant::from(url),
                Variant::from(client_id),
                Variant::from(client_secret),
            ],
        );
    }

    #[func]
    fn _connect_internal(&mut self, url: String, client_id: String, client_secret: String) {
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

        self.tracker().bind_mut().set(
            url,
            client_id,
            client_secret,
            self.store_session_device,
            self.force_in_editor,
            self.disabled,
        );

        self.tracker()
            .clone()
            .bind_mut()
            .set_global_properties(global_properties);

        if self.tracker().bind().is_disabled() {
            if Os::singleton().has_feature("engine") {
                godot_print!(
                    "OpenPanel Analytics are disabled while running in engine\nYou can enable them by calling Analytics.force_in_editor() in your code"
                );
            } else {
                godot_print!("OpenPanel Analytics are disabled");
            }
        } else {
            let tracker = self.tracker().clone();
            godot::task::spawn(async {
                if !Self::_connect_async(tracker).await {
                    godot_warn!("Failed to initialize analytics");
                }
            });
        }
    }

    async fn _connect_async(mut tracker: Gd<OpenPanelTracker>) -> bool {
        let mut tracker = tracker.bind_mut();
        let result = tracker.track("app_started", None, None).await;
        if let Ok(response) = result {
            if response.result == godot::classes::http_request::Result::SUCCESS
                && response.response_code >= 200
                && response.response_code < 300
            {
                godot_print!(
                    "Successfully tracked app start: {:?}",
                    tracker.get_device_id()
                );
                true
            } else {
                godot_error!(
                    "Failed to track app start (HTTP {}): {}",
                    response.response_code,
                    response.body.get_string_from_utf8()
                );
                false
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
            false
        }
    }

    fn _track_event_internal(
        &mut self,
        event: &str,
        profile_id: Option<String>,
        properties: Option<VarDictionary>,
        filter: Option<&dyn Fn(HashMap<String, String>) -> bool>,
    ) {
        if !self.tracker().bind().is_disabled() {
            if !self.tracker().bind().filter(properties.clone(), filter) {
                godot_print!("Analytics event '{}' was filtered out", event);
                return;
            }
            let mut tracker = self.tracker().clone();
            let event = event.to_owned();
            godot::task::spawn(async move {
                let result = tracker
                    .bind_mut()
                    .track(event.as_str(), profile_id, properties)
                    .await;
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
        self.tracker().bind_mut().force_in_editor(force);
    }

    #[allow(unused)]
    #[func]
    /// Disable sending events to OpenPanel
    pub fn disable(&mut self, disable: bool) {
        self.disabled = disable;
        self.tracker().bind_mut().disable(disable);
    }

    #[allow(unused)]
    #[func]
    pub fn is_disabled(&self) -> bool {
        if let Some(tracker) = self.tracker.clone() {
            tracker.bind().is_disabled()
        } else {
            false
        }
    }

    #[allow(unused)]
    #[func]
    pub fn track_event(&mut self, event: String, properties: Variant) {
        self._track_event_internal(
            event.as_str(),
            None,
            if properties != Variant::nil() {
                Some(properties.to::<VarDictionary>())
            } else {
                None
            },
            None,
        );
    }

    #[allow(unused)]
    pub fn track_event_with_properties(
        &mut self,
        event: String,
        properties: HashMap<String, String>,
    ) {
        self._track_event_internal(
            event.as_str(),
            None,
            Some(hashmap_to_dict(properties)),
            None,
        );
    }

    #[allow(unused)]
    #[func]
    pub fn track_event_bare(&mut self, event: String) {
        self._track_event_internal(event.as_str(), None, None, None);
    }

    #[allow(unused)]
    pub fn track_event_with_profile_id_and_properties(
        &mut self,
        event: String,
        profile_id: String,
        properties: HashMap<String, String>,
    ) {
        self._track_event_internal(
            event.as_str(),
            Some(profile_id),
            Some(hashmap_to_dict(properties)),
            None,
        );
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
            event.as_str(),
            Some(profile_id),
            if properties != Variant::nil() {
                Some(properties.to::<VarDictionary>())
            } else {
                None
            },
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
        self._track_event_internal(
            event.as_str(),
            None,
            properties.map(|p| hashmap_to_dict(p)),
            filter,
        );
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
            event.as_str(),
            profile_id,
            properties.map(|p| hashmap_to_dict(p)),
            filter,
        );
    }
}
