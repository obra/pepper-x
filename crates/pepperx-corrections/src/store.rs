use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::PathBuf;

const STORE_FILE_NAME: &str = "corrections.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreferredTranscription {
    pub source: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacementRule {
    pub source: String,
    pub replacement: String,
}

#[derive(Debug, Clone, Default)]
pub struct CorrectionStore {
    root: PathBuf,
    preferred_transcriptions: BTreeMap<String, String>,
    replacement_rules: BTreeMap<String, String>,
    pending_events: Vec<CorrectionEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CorrectionEvent {
    PreferredTranscription { source: String, replacement: String },
    ReplacementRule { source: String, replacement: String },
}

impl CorrectionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            ..Self::default()
        }
    }

    pub fn load(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        let store_path = root.join(STORE_FILE_NAME);

        if !store_path.exists() {
            return Ok(Self::new(root));
        }

        let data = fs::read_to_string(&store_path)?;
        let mut store = Self::new(root);

        for line in data.lines().filter(|line| !line.trim().is_empty()) {
            let event: CorrectionEvent = serde_json::from_str(line)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            store.apply_event_to_state(&event);
        }

        Ok(store)
    }

    pub fn set_preferred_transcription(
        &mut self,
        source: impl Into<String>,
        replacement: impl Into<String>,
    ) {
        let source = source.into();
        let replacement = replacement.into();
        self.apply_event(CorrectionEvent::PreferredTranscription {
            source,
            replacement,
        });
    }

    pub fn add_replacement_rule(
        &mut self,
        source: impl Into<String>,
        replacement: impl Into<String>,
    ) {
        let source = source.into();
        let replacement = replacement.into();
        self.apply_event(CorrectionEvent::ReplacementRule {
            source,
            replacement,
        });
    }

    pub fn prompt_memory_text(&self) -> Option<String> {
        if self.preferred_transcriptions.is_empty() && self.replacement_rules.is_empty() {
            return None;
        }

        let mut lines = Vec::new();

        if !self.preferred_transcriptions.is_empty() {
            lines.push(String::from("Preferred transcript examples:"));
            for (source, replacement) in &self.preferred_transcriptions {
                lines.push(format!("- raw: {source}"));
                lines.push(format!("  preferred: {replacement}"));
            }
        }

        if !self.replacement_rules.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(String::from("Preferred replacements:"));
            for (source, replacement) in &self.replacement_rules {
                lines.push(format!("- {source} => {replacement}"));
            }
        }

        Some(lines.join("\n"))
    }

    pub fn persist(&mut self) -> io::Result<()> {
        if self.pending_events.is_empty() {
            return Ok(());
        }

        fs::create_dir_all(&self.root)?;
        let store_path = self.root.join(STORE_FILE_NAME);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(store_path)?;

        for event in &self.pending_events {
            let json =
                serde_json::to_string(event).expect("correction store event should serialize");
            writeln!(file, "{json}")?;
        }

        self.pending_events.clear();
        Ok(())
    }

    fn apply_event(&mut self, event: CorrectionEvent) {
        self.apply_event_to_state(&event);
        self.pending_events.push(event);
    }

    fn apply_event_to_state(&mut self, event: &CorrectionEvent) {
        match event {
            CorrectionEvent::PreferredTranscription {
                source,
                replacement,
            } => {
                self.preferred_transcriptions
                    .insert(source.clone(), replacement.clone());
            }
            CorrectionEvent::ReplacementRule {
                source,
                replacement,
            } => {
                self.replacement_rules
                    .insert(source.clone(), replacement.clone());
            }
        }
    }
}
