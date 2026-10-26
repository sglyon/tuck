//! Persisted global dictation preferences and transactional shortcut replacement.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
use tuck_core::{ports::Result, AppError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub shortcut: String,
    pub auto_paste: bool,
    #[serde(default)]
    pub cleanup_enabled: bool,
    #[serde(default = "default_cleanup_prompt")]
    pub cleanup_prompt: String,
}
fn default_cleanup_prompt() -> String {
    tuck_core::dictation_cleanup::DEFAULT_PROMPT.to_owned()
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: crate::global_dictation::SHORTCUT.into(),
            auto_paste: true,
            cleanup_enabled: false,
            cleanup_prompt: default_cleanup_prompt(),
        }
    }
}
impl Config {
    pub fn from_value(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|_| AppError::invalid("Invalid dictation settings."))?;
        if config.cleanup_prompt.trim().is_empty()
            || config.cleanup_prompt.chars().count()
                > tuck_core::dictation_cleanup::MAX_PROMPT_CHARS
        {
            return Err(AppError::invalid(
                "Enter a cleanup prompt of 1–2,000 characters.",
            ));
        }
        if config.shortcut.len() > 100 {
            return Err(AppError::invalid("The shortcut is too long."));
        }
        let key = config.key()?;
        let function_key = matches!(
            key.key,
            Code::F1
                | Code::F2
                | Code::F3
                | Code::F4
                | Code::F5
                | Code::F6
                | Code::F7
                | Code::F8
                | Code::F9
                | Code::F10
                | Code::F11
                | Code::F12
                | Code::F13
                | Code::F14
                | Code::F15
                | Code::F16
                | Code::F17
                | Code::F18
                | Code::F19
                | Code::F20
                | Code::F21
                | Code::F22
                | Code::F23
                | Code::F24
        );
        if !function_key
            && !key
                .mods
                .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
        {
            return Err(AppError::invalid(
                "Use a function key on its own, or add Command, Control, or Option/Alt.",
            ));
        }
        if key.id()
            == Shortcut::from_str("CommandOrControl+Shift+R")
                .expect("recording shortcut")
                .id()
        {
            return Err(AppError::invalid(
                "That shortcut starts a meeting. Choose another shortcut for dictation.",
            ));
        }
        Ok(config)
    }
    pub fn key(&self) -> Result<Shortcut> {
        Shortcut::from_str(&self.shortcut)
            .map_err(|_| AppError::invalid("Choose a valid function key or a key with modifiers."))
    }
}

pub trait Registry {
    fn contains(&self, key: Shortcut) -> bool;
    fn register(&self, key: Shortcut) -> Result<()>;
    fn unregister(&self, key: Shortcut) -> Result<()>;
}
/// Register the replacement before releasing the previous shortcut. A conflict or
/// failed settings write leaves the previous configuration usable.
pub fn apply(
    old: &Config,
    new: &Config,
    registry: &impl Registry,
    save: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    let old_key = old.key()?;
    let new_key = new.key()?;
    let old_registered = old.enabled && registry.contains(old_key);
    let same = old_key.id() == new_key.id();
    let add = new.enabled && !(same && old_registered);
    let remove = old_registered && (!new.enabled || !same);
    if add {
        registry.register(new_key)?;
    }
    if remove {
        if let Err(error) = registry.unregister(old_key) {
            if add {
                registry.unregister(new_key)?;
            }
            return Err(error);
        }
    }
    if let Err(error) = save() {
        if add {
            registry.unregister(new_key)?;
        }
        if remove {
            registry.register(old_key)?;
        }
        return Err(error);
    }
    Ok(new.enabled && registry.contains(new_key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashSet};
    struct Fake {
        keys: RefCell<HashSet<u32>>,
        conflict: Option<u32>,
    }
    impl Registry for Fake {
        fn contains(&self, key: Shortcut) -> bool {
            self.keys.borrow().contains(&key.id())
        }
        fn register(&self, key: Shortcut) -> Result<()> {
            if self.conflict == Some(key.id()) {
                return Err(AppError::unavailable("Shortcut in use."));
            }
            self.keys.borrow_mut().insert(key.id());
            Ok(())
        }
        fn unregister(&self, key: Shortcut) -> Result<()> {
            self.keys.borrow_mut().remove(&key.id());
            Ok(())
        }
    }
    #[test]
    fn conflicts_and_save_failures_keep_the_previous_shortcut() {
        let old = Config::default();
        let new = Config {
            shortcut: "Alt+Shift+D".into(),
            ..old.clone()
        };
        let f = Fake {
            keys: RefCell::new(HashSet::from([old.key().unwrap().id()])),
            conflict: Some(new.key().unwrap().id()),
        };
        assert!(apply(&old, &new, &f, || panic!("conflict must not save")).is_err());
        assert!(f.contains(old.key().unwrap()));
        let f = Fake {
            conflict: None,
            ..f
        };
        assert!(apply(&old, &new, &f, || Err(AppError::unavailable("disk full"))).is_err());
        assert!(f.contains(old.key().unwrap()));
        assert!(!f.contains(new.key().unwrap()));
    }
    #[test]
    fn disable_releases_shortcut_and_reenable_can_retry() {
        let old = Config::default();
        let off = Config {
            enabled: false,
            ..old.clone()
        };
        let f = Fake {
            keys: RefCell::new(HashSet::from([old.key().unwrap().id()])),
            conflict: None,
        };
        assert!(!apply(&old, &off, &f, || Ok(())).unwrap());
        assert!(f.keys.borrow().is_empty());
        assert!(apply(&off, &old, &f, || Ok(())).unwrap());
    }
    #[test]
    fn function_keys_work_without_modifiers() {
        for shortcut in ["F1", "F12", "F13", "F20", "F24"] {
            let config = Config::from_value(
                &serde_json::json!({"enabled":true,"shortcut":shortcut,"autoPaste":false}),
            )
            .unwrap();
            assert_eq!(config.shortcut, shortcut);
        }
    }
    #[test]
    fn validates_modifiers_and_reserves_the_meeting_shortcut() {
        for shortcut in ["Space", "Shift+D", "CommandOrControl+Shift+R", "wat+KeyD"] {
            assert!(Config::from_value(
                &serde_json::json!({"enabled":true,"shortcut":shortcut,"autoPaste":false})
            )
            .is_err());
        }
        let config = Config {
            auto_paste: false,
            ..Config::default()
        };
        assert_eq!(
            Config::from_value(&serde_json::to_value(&config).unwrap()).unwrap(),
            config
        );
    }
}
