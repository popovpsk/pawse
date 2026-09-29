mod presets;

use std::collections::BTreeSet;

use regex::{NoExpand, Regex};
use serde::{Deserialize, Serialize};

use crate::store::Love;
use crate::{NowPlaying, Scrobble};

pub use presets::{PRESETS, Preset, PresetRule, preset};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Artist,
    #[default]
    Title,
    Album,
    AlbumArtist,
}

impl Field {
    pub const ALL: [Field; 4] = [
        Field::Artist,
        Field::Title,
        Field::Album,
        Field::AlbumArtist,
    ];
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub field: Field,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub replacement: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub ignore_case: bool,
}

impl Default for CustomRule {
    fn default() -> Self {
        Self {
            enabled: true,
            field: Field::Title,
            pattern: String::new(),
            replacement: String::new(),
            regex: false,
            ignore_case: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteConfig {
    #[serde(default)]
    pub presets: BTreeSet<String>,
    #[serde(default)]
    pub rules: Vec<CustomRule>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleError {
    pub index: usize,
    pub message: String,
}

struct Step {
    fields: Vec<Field>,
    regex: Regex,
    replacement: String,
    expand: bool,
    all: bool,
}

impl Step {
    fn run(&self, text: &str) -> Option<String> {
        if !self.regex.is_match(text) {
            return None;
        }
        let limit = if self.all { 0 } else { 1 };
        let out = if self.expand {
            self.regex
                .replacen(text, limit, self.replacement.as_str())
                .into_owned()
        } else {
            self.regex
                .replacen(text, limit, NoExpand(&self.replacement))
                .into_owned()
        };
        (out != text).then_some(out)
    }
}

#[derive(Default)]
pub struct Rewriter {
    steps: Vec<Step>,
}

impl Rewriter {
    pub fn compile(config: &RewriteConfig) -> (Self, Vec<RuleError>) {
        let mut steps = Vec::new();
        for preset in PRESETS.iter().filter(|p| config.presets.contains(p.id)) {
            steps.extend(preset_steps(preset));
        }
        let mut errors = Vec::new();
        for (index, rule) in config.rules.iter().enumerate() {
            if !rule.enabled || rule.pattern.is_empty() {
                continue;
            }
            match rule_step(rule) {
                Ok(step) => steps.push(step),
                Err(message) => errors.push(RuleError { index, message }),
            }
        }
        (Self { steps }, errors)
    }

    pub fn for_preset(preset: &Preset) -> Self {
        Self {
            steps: preset_steps(preset),
        }
    }

    pub fn for_rule(rule: &CustomRule) -> Result<Self, String> {
        if rule.pattern.is_empty() {
            return Ok(Self::default());
        }
        rule_step(rule).map(|step| Self { steps: vec![step] })
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn touches(&self, field: Field) -> bool {
        self.steps.iter().any(|s| s.fields.contains(&field))
    }

    pub fn rewrite(&self, field: Field, text: &str) -> Option<String> {
        let mut current: Option<String> = None;
        for step in self.steps.iter().filter(|s| s.fields.contains(&field)) {
            let input = current.as_deref().unwrap_or(text);
            if let Some(out) = step.run(input) {
                current = Some(out);
            }
        }
        let out = current?.trim().to_string();
        (out != text).then_some(out)
    }

    fn required(&self, field: Field, text: &str) -> String {
        match self.rewrite(field, text) {
            Some(out) if !out.is_empty() => out,
            _ => text.to_string(),
        }
    }

    fn optional(&self, field: Field, text: &Option<String>) -> Option<String> {
        let text = text.as_deref()?;
        match self.rewrite(field, text) {
            Some(out) if out.is_empty() => None,
            Some(out) => Some(out),
            None => Some(text.to_string()),
        }
    }

    pub fn apply(&self, scrobble: &Scrobble) -> Scrobble {
        Scrobble {
            artist: self.required(Field::Artist, &scrobble.artist),
            title: self.required(Field::Title, &scrobble.title),
            album: self.optional(Field::Album, &scrobble.album),
            album_artist: self.optional(Field::AlbumArtist, &scrobble.album_artist),
            ..scrobble.clone()
        }
    }

    pub fn apply_now_playing(&self, now_playing: &NowPlaying) -> NowPlaying {
        NowPlaying {
            artist: self.required(Field::Artist, &now_playing.artist),
            title: self.required(Field::Title, &now_playing.title),
            album: self.optional(Field::Album, &now_playing.album),
            album_artist: self.optional(Field::AlbumArtist, &now_playing.album_artist),
            ..now_playing.clone()
        }
    }

    pub fn apply_love(&self, love: &Love) -> Love {
        Love {
            artist: self.required(Field::Artist, &love.artist),
            title: self.required(Field::Title, &love.title),
            ..love.clone()
        }
    }
}

fn preset_steps(preset: &Preset) -> Vec<Step> {
    preset
        .rules
        .iter()
        .filter_map(|rule| match Regex::new(rule.pattern) {
            Ok(regex) => Some(Step {
                fields: preset.fields.to_vec(),
                regex,
                replacement: rule.replacement.to_string(),
                expand: true,
                all: false,
            }),
            Err(e) => {
                log::error!("scrobble: preset {} has a broken pattern: {e}", preset.id);
                None
            }
        })
        .collect()
}

fn rule_step(rule: &CustomRule) -> Result<Step, String> {
    let body = if rule.regex {
        rule.pattern.clone()
    } else {
        regex::escape(&rule.pattern)
    };
    let pattern = if rule.ignore_case {
        format!("(?i){body}")
    } else {
        body
    };
    let regex = Regex::new(&pattern).map_err(|e| e.to_string())?;
    Ok(Step {
        fields: vec![rule.field],
        regex,
        replacement: if rule.regex {
            brace_group_numbers(&rule.replacement)
        } else {
            rule.replacement.clone()
        },
        expand: rule.regex,
        all: true,
    })
}

fn brace_group_numbers(replacement: &str) -> String {
    let mut out = String::with_capacity(replacement.len() + 4);
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                out.push_str("$$");
                chars.next();
            }
            Some(d) if d.is_ascii_digit() => {
                out.push_str("${");
                while let Some(d) = chars.next_if(char::is_ascii_digit) {
                    out.push(d);
                }
                out.push('}');
            }
            _ => out.push('$'),
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
}

impl Sample {
    fn field(&self, field: Field) -> Option<&str> {
        match field {
            Field::Artist => Some(&self.artist),
            Field::Title => Some(&self.title),
            Field::Album => self.album.as_deref(),
            Field::AlbumArtist => self.album_artist.as_deref(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub tracks: usize,
    pub examples: Vec<(String, String)>,
}

pub fn preview(rewriter: &Rewriter, samples: &[Sample], max_examples: usize) -> Preview {
    let mut result = Preview::default();
    if rewriter.is_empty() {
        return result;
    }
    let fields: Vec<Field> = Field::ALL
        .into_iter()
        .filter(|f| rewriter.touches(*f))
        .collect();
    for sample in samples {
        let mut changed = false;
        for field in &fields {
            let Some(text) = sample.field(*field) else {
                continue;
            };
            let Some(out) = rewriter.rewrite(*field, text) else {
                continue;
            };
            changed = true;
            if result.examples.len() < max_examples
                && !result.examples.iter().any(|(before, _)| before == text)
            {
                result.examples.push((text.to_string(), out));
            }
        }
        if changed {
            result.tracks += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests;
