#![allow(dead_code)]

use godot::classes::{EditorPlugin, IEditorPlugin, Os, ProjectSettings};
use godot::obj::{BaseRef, WithSignals};
use godot::prelude::*;
use godot::tools::try_get_autoload_by_name;

use crate::tracker::{OpenPanelTracker, hashmap_to_dict};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(GodotClass)]
#[class(tool, init, base=EditorPlugin)]
pub struct AnalyticsPlugin {
    base: Base<EditorPlugin>,
}

#[godot_api]
impl IEditorPlugin for AnalyticsPlugin {
    fn enter_tree(&mut self) {
        if self
            .base()
            .get_tree()
            .get_root()
            .find_child("OpenPanel")
            .is_none()
        {
            self.base_mut()
                .add_autoload_singleton("OpenPanel", "res://addons/OpenPanel/OpenPanel.tscn");
        }
    }

    fn exit_tree(&mut self) {
        self.base_mut().remove_autoload_singleton("OpenPanel");
    }
}

thread_local! {
    static INSTANCE: RefCell<Option<Gd<Analytics>>> = RefCell::new(None);
}

#[derive(GodotClass)]
#[class(init, base=Object)]
pub struct Analytics {
    tracker: Option<Gd<OpenPanelTracker>>,
    device_id: Variant,
    store_session_device: bool,
    force_in_editor: bool,
    disabled: bool,
    base: Base<Object>,
}

pub fn try_to_string_dict(var_dict: &Variant) -> Option<Dictionary<GString, GString>> {
    let mut success = true;
    if let Ok(dict) = var_dict.try_to::<Dictionary<Variant, Variant>>() {
        let result = dict
            .into_iter()
            .filter_map(|(k, v)| {
                if let (Ok(k), Ok(v)) = (k.try_to::<GString>(), v.try_to::<GString>()) {
                    Some((k, v))
                } else {
                    success = false;
                    None
                }
            })
            .collect::<Dictionary<GString, GString>>();
        if success { Some(result) } else { None }
    } else {
        None
    }
}

#[godot_api]
impl Analytics {
    #[func]
    pub fn with_tracker(
        tracker: Gd<OpenPanelTracker>,
        store_session_device: bool,
        force_in_editor: bool,
        disabled: bool,
    ) -> Gd<Self> {
        let new_instance = Gd::from_init_fn(|base| {
            // Accept a base of type Base<Node3D> and directly forward it.
            Self {
                tracker: Some(tracker),
                device_id: Variant::nil(),
                store_session_device,
                force_in_editor,
                disabled,
                base,
            }
        });

        INSTANCE.with(|cell| cell.replace(Some(new_instance.clone())));
        new_instance
    }

    pub fn tracker(&mut self) -> Option<Gd<OpenPanelTracker>> {
        if self.tracker.is_none() {
            let tracker = try_get_autoload_by_name::<OpenPanelTracker>("OpenPanel");
            if let Ok(tracker) = tracker {
                self.tracker.replace(tracker.clone());
            } else {
                godot_error!(
                    "Analytics tracker not found. Please check if the OpenPanel autoload was removed from Project Settings."
                );
            }
        }

        self.tracker.clone()
    }

    pub fn clear_tracker(tracker: Gd<OpenPanelTracker>) {
        Self::with_instance(|analytics| {
            if let Some(current) = analytics.tracker.clone() {
                if current.instance_id() == tracker.instance_id() {
                    analytics.tracker = None;
                }
            }
        });
    }

    fn with_instance<F, R>(mut f: F) -> Option<R>
    where
        F: FnMut(&mut Analytics) -> R,
    {
        INSTANCE.with(|cell| {
            let mut gd = cell.borrow().clone()?;
            let mut guard = gd.bind_mut();
            Some(f(&mut guard))
        })
    }

    pub fn store(&mut self) {
        INSTANCE.with(|cell| cell.replace(Some(self.to_gd().clone())));
    }

    #[func]
    pub fn openpanel_connect(url: String, client_id: String, client_secret: String) {
        _ = Self::with_instance(|analytics| {
            analytics._openpanel_connect(url.clone(), client_id.clone(), client_secret.clone());
        });
    }

    #[func]
    fn _openpanel_connect(&mut self, url: String, client_id: String, client_secret: String) {
        self.disabled = false;

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

        if let Some(mut tracker) = self.tracker() {
            {
                let mut tracker_binding = tracker.bind_mut();
                tracker_binding.set(
                    url,
                    client_id,
                    client_secret,
                    self.store_session_device,
                    self.force_in_editor,
                    self.disabled,
                );
                tracker_binding.set_global_properties(global_properties);
                if tracker_binding.is_disabled() {
                    if Os::singleton().has_feature("editor") {
                        godot_print!(
                            "OpenPanel Analytics are disabled while running in editor\nYou can enable them by calling Analytics.force_in_editor() in your code"
                        );
                    } else {
                        godot_print!("OpenPanel Analytics are disabled");
                    }
                } else {
                    tracker_binding.track("app_started", None, None);
                }
            }
        } else {
            godot_error!(
                "Analytics tracker not found. Please check if the OpenPanel autoload was removed from Project Settings."
            );
            return;
        }
    }

    pub fn set_device_id(device_id: &String) {
        Self::with_instance(|analytics| {
            analytics.device_id = Variant::from(device_id.as_str());
            analytics
                .signals()
                .device_id_updated()
                .emit(device_id.to_owned());
        });
    }

    #[signal]
    fn device_id_updated(new_device_id: String);

    pub fn connect_device_id_updated(node_ref: BaseRef<'_, impl WithSignals>, func: &str) {
        _ = Self::with_instance(move |analytics| {
            let callable = node_ref.callable(func);
            analytics.base_mut().connect("device_id_updated", &callable)
        });
    }

    #[func]
    pub fn get_device_id() -> Variant {
        Self::with_instance(|analytics| analytics.device_id.clone()).unwrap_or_default()
    }

    fn _track_event_internal(
        &mut self,
        event: &str,
        profile_id: Option<String>,
        properties: Option<Dictionary<GString, GString>>,
        filter: Option<&dyn Fn(HashMap<String, String>) -> bool>,
    ) {
        if let Some(tracker) = self.tracker() {
            if tracker.bind().is_disabled() {
                return;
            }
            if !tracker.bind().filter(properties.clone(), filter) {
                godot_print!("Analytics event '{}' was filtered out", event);
                return;
            }

            let mut tracker = tracker.to_godot_owned();
            let event = event.to_owned();
            godot::task::spawn(async move {
                tracker
                    .bind_mut()
                    .track(event.as_str(), profile_id, properties);
            });
        } else {
            godot_error!("Analytics tracker not initialized");
        }
    }

    #[func]
    /// Force enable analytics while running in the editor (for testing)
    pub fn force_in_editor(force: bool) {
        _ = Self::with_instance(|analytics| {
            analytics._force_in_editor(force);
        });
    }
    pub fn _force_in_editor(&mut self, force: bool) {
        self.force_in_editor = force;
        if let Some(mut tracker) = self.tracker() {
            tracker.bind_mut().force_in_editor(force);
        }
    }

    #[func]
    /// Disable sending events to OpenPanel
    pub fn disable(disable: bool) {
        _ = Self::with_instance(|analytics| {
            analytics._disable(disable);
        });
    }
    pub fn _disable(&mut self, disable: bool) {
        self.disabled = disable;
        if let Some(mut tracker) = self.tracker() {
            tracker.bind_mut().disable(disable);
        }
    }

    #[func]
    pub fn is_disabled() -> bool {
        Self::with_instance(|analytics| analytics._is_disabled()).unwrap_or(false)
    }
    pub fn _is_disabled(&mut self) -> bool {
        if let Some(tracker) = self.tracker.clone() {
            tracker.bind().is_disabled()
        } else {
            false
        }
    }

    #[func]
    pub fn track_event(event: GString, properties: Variant) {
        Self::track(&event.to_string(), properties);
    }

    pub fn track(event: &str, properties: Variant) {
        _ = Self::with_instance(|analytics| {
            analytics._track_event_internal(event, None, try_to_string_dict(&properties), None);
        });
    }

    pub fn track_event_with_properties(
        &mut self,
        event: &str,
        properties: HashMap<String, String>,
    ) {
        self._track_event_internal(event, None, Some(hashmap_to_dict(properties)), None);
    }

    #[func]
    pub fn track_event_bare(&mut self, event: String) {
        self._track_event_internal(event.as_str(), None, None, None);
    }

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
            try_to_string_dict(&properties),
            None,
        );
    }

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
