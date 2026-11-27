//! Signals: values a plugin reports about the user's context (the file types selected in the
//! file manager, a timeline's type, the scene in a DAW). KnobDeck keeps the last value of each
//! and checks it against the conditions of the profiles' auto-open rules.
//!
//! Signals are pushed, never read on demand: a plugin watches its source and sends a `signal`
//! frame when the value changes (and once when it starts watching). If a value can't be had from
//! an event or a hook, it is not a signal.
//!
//! A plugin declares its signals in `manifest.json` under `signals` (the host reads them while
//! the plugin is not running, to build the rule editor), then reports values with the `signal`
//! frame. This is not the host-to-plugin [`events`](crate::events) list: those tell a plugin what
//! happened in KnobDeck; a signal is what the plugin knows about the user's world.
//!
//! Signal ids are `<namespace>.<name>` (`files.extension`), lowercase, and not tied to a plugin
//! id: a plugin ships per OS (`files-windows`), but a saved rule must work on every OS.
//! `core.` is the host's own (`core.window.app`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// Longest signal value the host accepts (JSON text, bytes). Longer ones are dropped.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

/// What a signal's value is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// Open-ended text; a rule matches it as a substring or `/regex/`.
    #[default]
    Text,
    /// One of [`SignalDecl::options`]; a value outside them is dropped.
    Enum,
    Number,
    Bool,
}

/// One allowed value of an `enum` signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SignalOption {
    pub value: String,
    /// Shown in the rule editor; defaults to `value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One entry of a manifest's `signals`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SignalDecl {
    /// `<namespace>.<name>`, lowercase.
    pub id: String,
    /// Shown in the rule editor.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: SignalKind,
    /// The value is a set (an array): several files with different extensions are selected.
    #[serde(default, skip_serializing_if = "is_false")]
    pub multi: bool,
    /// The allowed values of an `enum` signal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SignalOption>,
    /// OSes the signal exists on (`windows`, `macos`, `linux`). Empty: every OS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl SignalDecl {
    /// A declaration of `kind`, pushed by the plugin, valid on every OS.
    pub fn new(id: &str, label: &str, kind: SignalKind) -> Self {
        SignalDecl { id: id.into(), label: label.into(), description: None, kind, multi: false, options: vec![], platforms: vec![] }
    }
    /// An `enum` signal with these `(value, label)` options.
    pub fn choice(id: &str, label: &str, options: &[(&str, &str)]) -> Self {
        let mut d = Self::new(id, label, SignalKind::Enum);
        d.options = options.iter().map(|(v, l)| SignalOption { value: (*v).into(), label: Some((*l).into()) }).collect();
        d
    }
    /// The value is a set.
    pub fn multi(mut self) -> Self {
        self.multi = true;
        self
    }
    pub fn description(mut self, text: &str) -> Self {
        self.description = Some(text.into());
        self
    }
    /// Only on these OSes (`windows`, `macos`, `linux`).
    pub fn platforms(mut self, platforms: &[&str]) -> Self {
        self.platforms = platforms.iter().map(|p| (*p).into()).collect();
        self
    }

    /// Whether `id` is a well-formed signal id: `<namespace>.<name>[.<name>…]`, each part
    /// lowercase letters, digits, `_` or `-`.
    pub fn valid_id(id: &str) -> bool {
        let mut parts = id.split('.');
        let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        id.contains('.') && parts.all(ok)
    }

    /// The value as this declaration allows it: the same shape, with an `enum`'s values checked
    /// against the options (case-insensitively, returned as the option spells it) and a scalar
    /// wrapped when `multi`. `None` when it does not fit or is too long.
    pub fn accept(&self, value: &Value) -> Option<Value> {
        if value.to_string().len() > MAX_VALUE_BYTES {
            return None;
        }
        let one = |v: &Value| -> Option<Value> {
            match self.kind {
                SignalKind::Text => v.as_str().map(|s| Value::from(s.to_owned())),
                SignalKind::Enum => {
                    let s = v.as_str()?;
                    self.options.iter().find(|o| o.value.eq_ignore_ascii_case(s)).map(|o| Value::from(o.value.clone()))
                }
                SignalKind::Number => v.is_number().then(|| v.clone()),
                SignalKind::Bool => v.is_boolean().then(|| v.clone()),
            }
        };
        if self.multi {
            let items: Vec<Value> = match value {
                Value::Array(a) => a.iter().map(one).collect::<Option<_>>()?,
                v => vec![one(v)?],
            };
            Some(Value::Array(items))
        } else {
            one(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decl(kind: SignalKind, multi: bool) -> SignalDecl {
        SignalDecl {
            id: "files.extension".into(),
            label: "File type".into(),
            description: None,
            kind,
            multi,
            options: ["pdf", "png"].map(|v| SignalOption { value: v.into(), label: None }).into(),
            platforms: vec![],
        }
    }

    #[test]
    fn ids() {
        assert!(SignalDecl::valid_id("files.extension"));
        assert!(SignalDecl::valid_id("core.window.app"));
        assert!(!SignalDecl::valid_id("files"));
        assert!(!SignalDecl::valid_id("Files.ext"));
        assert!(!SignalDecl::valid_id("a..b"));
    }

    #[test]
    fn enum_values_must_be_declared() {
        let d = decl(SignalKind::Enum, true);
        assert_eq!(d.accept(&json!(["PDF", "png"])), Some(json!(["pdf", "png"])));
        assert_eq!(d.accept(&json!(["pdf", "exe"])), None, "one undeclared value drops the report");
        assert_eq!(d.accept(&json!("pdf")), Some(json!(["pdf"])), "a scalar becomes a set");
        assert_eq!(d.accept(&json!([])), Some(json!([])), "reported, nothing");
        assert_eq!(decl(SignalKind::Enum, false).accept(&json!(["pdf"])), None);
    }

    #[test]
    fn other_kinds() {
        assert_eq!(decl(SignalKind::Text, false).accept(&json!("x")), Some(json!("x")));
        assert_eq!(decl(SignalKind::Text, false).accept(&json!(1)), None);
        assert_eq!(decl(SignalKind::Number, false).accept(&json!(2.5)), Some(json!(2.5)));
        assert_eq!(decl(SignalKind::Bool, false).accept(&json!(true)), Some(json!(true)));
        assert_eq!(decl(SignalKind::Bool, false).accept(&json!("true")), None);
    }

    #[test]
    fn too_long_is_dropped() {
        let big = json!("x".repeat(MAX_VALUE_BYTES));
        assert_eq!(decl(SignalKind::Text, false).accept(&big), None);
    }
}
