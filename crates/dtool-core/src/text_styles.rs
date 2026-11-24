use dtool_protocol::CommandError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleKind {
    Character,
    Paragraph,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedTextStyle {
    pub name: String,
    pub kind: StyleKind,
    #[serde(default)]
    pub based_on: Option<String>,
    #[serde(default)]
    pub props: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_style: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextStyleStore {
    #[serde(default)]
    pub styles: Vec<NamedTextStyle>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppliedStyle {
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub overrides: Map<String, Value>,
}

fn merge(target: &mut Map<String, Value>, patch: &Map<String, Value>) {
    for (key, value) in patch {
        target.insert(key.clone(), value.clone());
    }
}

impl TextStyleStore {
    pub fn get(&self, kind: StyleKind, name: &str) -> Option<&NamedTextStyle> {
        self.styles.iter().find(|style| style.kind == kind && style.name == name)
    }

    pub fn names(&self, kind: StyleKind) -> Vec<String> {
        self.styles.iter().filter(|style| style.kind == kind).map(|style| style.name.clone()).collect()
    }

    fn reaches(&self, kind: StyleKind, from: &str, target: &str) -> bool {
        let mut current = Some(from.to_string());
        for _ in 0..=MAX_DEPTH {
            let Some(name) = current else { return false };
            if name == target {
                return true;
            }
            current = self.get(kind, &name).and_then(|style| style.based_on.clone());
        }
        true
    }

    pub fn define(&mut self, style: NamedTextStyle) -> Result<(), CommandError> {
        if style.name.trim().is_empty() {
            return Err(CommandError::invalid_params());
        }
        if let Some(parent) = &style.based_on {
            self.get(style.kind, parent).ok_or_else(CommandError::not_found)?;
            if self.reaches(style.kind, parent, &style.name) {
                return Err(CommandError::new("STYLE_CYCLE"));
            }
        }
        match self.styles.iter_mut().find(|existing| existing.kind == style.kind && existing.name == style.name) {
            Some(existing) => *existing = style,
            None => self.styles.push(style),
        }
        Ok(())
    }

    pub fn define_from_selection(&mut self, name: &str, kind: StyleKind, based_on: Option<&str>, current: &Map<String, Value>) -> Result<(), CommandError> {
        let inherited = match based_on {
            Some(parent) => self.resolve(kind, parent)?,
            None => Map::new(),
        };
        let props = current
            .iter()
            .filter(|(key, value)| inherited.get(*key) != Some(*value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        self.define(NamedTextStyle { name: name.to_string(), kind, based_on: based_on.map(str::to_string), props, next_style: None })
    }

    pub fn remove(&mut self, kind: StyleKind, name: &str) -> Result<NamedTextStyle, CommandError> {
        let index = self.styles.iter().position(|style| style.kind == kind && style.name == name).ok_or_else(CommandError::not_found)?;
        let removed = self.styles.remove(index);
        let resolved = {
            let mut props = match &removed.based_on {
                Some(parent) => self.resolve(kind, parent)?,
                None => Map::new(),
            };
            merge(&mut props, &removed.props);
            props
        };
        for child in self.styles.iter_mut().filter(|style| style.kind == kind && style.based_on.as_deref() == Some(name)) {
            let mut props = resolved.clone();
            merge(&mut props, &child.props);
            child.props = props;
            child.based_on = removed.based_on.clone();
        }
        Ok(removed)
    }

    pub fn resolve(&self, kind: StyleKind, name: &str) -> Result<Map<String, Value>, CommandError> {
        let mut chain = Vec::new();
        let mut current = Some(name.to_string());
        while let Some(name) = current {
            if chain.len() > MAX_DEPTH {
                return Err(CommandError::new("STYLE_CYCLE"));
            }
            let style = self.get(kind, &name).ok_or_else(CommandError::not_found)?;
            current = style.based_on.clone();
            chain.push(style);
        }
        let mut props = Map::new();
        for style in chain.iter().rev() {
            merge(&mut props, &style.props);
        }
        Ok(props)
    }

    pub fn effective(&self, kind: StyleKind, base: &Value, applied: &AppliedStyle) -> Result<Value, CommandError> {
        let mut props = base.as_object().cloned().ok_or_else(CommandError::invalid_params)?;
        if let Some(name) = &applied.style {
            merge(&mut props, &self.resolve(kind, name)?);
        }
        merge(&mut props, &applied.overrides);
        Ok(Value::Object(props))
    }

    pub fn apply(&self, kind: StyleKind, name: &str, keep_overrides: bool, applied: &AppliedStyle) -> Result<AppliedStyle, CommandError> {
        self.get(kind, name).ok_or_else(CommandError::not_found)?;
        Ok(AppliedStyle {
            style: Some(name.to_string()),
            overrides: if keep_overrides { applied.overrides.clone() } else { Map::new() },
        })
    }
}

impl AppliedStyle {
    pub fn set_override(&mut self, key: &str, value: Value) {
        self.overrides.insert(key.to_string(), value);
    }

    pub fn clear_override(&mut self, key: Option<&str>) -> bool {
        match key {
            Some(key) => self.overrides.remove(key).is_some(),
            None => {
                let had = !self.overrides.is_empty();
                self.overrides.clear();
                had
            }
        }
    }

    pub fn has_override(&self) -> bool {
        !self.overrides.is_empty()
    }

    pub fn from_pairs(style: Option<String>, pairs: &[(String, String)]) -> Self {
        let overrides = pairs.iter().map(|(k, v)| (k.clone(), serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.clone())))).collect();
        Self { style, overrides }
    }

    pub fn to_pairs(&self) -> Vec<(String, String)> {
        self.overrides.iter().map(|(k, v)| (k.clone(), v.to_string())).collect()
    }

    pub fn write_range(&self, kind: StyleKind, range: &mut Value) {
        let Some(fields) = range.as_object_mut() else {
            return;
        };
        let key = style_key(kind);
        match &self.style {
            Some(name) => {
                fields.insert(key.to_string(), Value::String(name.clone()));
            }
            None => {
                fields.remove(key);
            }
        }
        if self.overrides.is_empty() || self.style.is_none() {
            fields.remove(STYLE_OVERRIDES_KEY);
        } else {
            fields.insert(STYLE_OVERRIDES_KEY.to_string(), Value::Object(self.overrides.clone()));
        }
    }

    pub fn read_range(kind: StyleKind, range: &Value) -> Self {
        let style = range.get(style_key(kind)).and_then(Value::as_str).map(str::to_string);
        let overrides = if style.is_some() { range.get(STYLE_OVERRIDES_KEY).and_then(Value::as_object).cloned().unwrap_or_default() } else { Map::new() };
        Self { style, overrides }
    }
}

pub const CHAR_STYLE_KEY: &str = "char_style";
pub const PARA_STYLE_KEY: &str = "para_style";
pub const STYLE_OVERRIDES_KEY: &str = "style_overrides";

pub fn style_key(kind: StyleKind) -> &'static str {
    match kind {
        StyleKind::Character => CHAR_STYLE_KEY,
        StyleKind::Paragraph => PARA_STYLE_KEY,
    }
}