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

    /// Returns preferred transcriptions as a list of display strings (one per entry).
    pub fn preferred_transcriptions(&self) -> Vec<String> {
        self.preferred_transcriptions.values().cloned().collect()
    }

    /// Returns replacement rules formatted as "source -> replacement" strings.
    pub fn replacement_rules(&self) -> Vec<(String, String)> {
        self.replacement_rules
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Replaces all preferred transcriptions with the given list.
    /// Each entry is stored with a lowercased source key equal to its value.
    pub fn set_all_preferred_transcriptions(&mut self, entries: &[String]) {
        self.preferred_transcriptions.clear();
        for entry in entries {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            let source = trimmed.to_lowercase();
            self.apply_event(CorrectionEvent::PreferredTranscription {
                source,
                replacement: trimmed.to_string(),
            });
        }
    }

    /// Replaces all replacement rules with the given list of (source, replacement) pairs.
    pub fn set_all_replacement_rules(&mut self, rules: &[(String, String)]) {
        self.replacement_rules.clear();
        for (source, replacement) in rules {
            let source = source.trim().to_string();
            let replacement = replacement.trim().to_string();
            if source.is_empty() || replacement.is_empty() {
                continue;
            }
            self.apply_event(CorrectionEvent::ReplacementRule {
                source,
                replacement,
            });
        }
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

    /// Clear the store file and rewrite it with the current in-memory state.
    /// Call this after using `set_all_preferred_transcriptions` / `set_all_replacement_rules`
    /// to compact the event log.
    pub fn rewrite(&mut self) -> io::Result<()> {
        // Snapshot current state
        let preferred: Vec<(String, String)> = self
            .preferred_transcriptions
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let rules: Vec<(String, String)> = self
            .replacement_rules
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Clear file and in-memory state
        self.clear()?;

        // Rebuild from snapshot
        for (source, replacement) in preferred {
            self.apply_event(CorrectionEvent::PreferredTranscription {
                source,
                replacement,
            });
        }
        for (source, replacement) in rules {
            self.apply_event(CorrectionEvent::ReplacementRule {
                source,
                replacement,
            });
        }

        self.persist()
    }

    pub fn clear(&mut self) -> io::Result<()> {
        let store_path = self.root.join(STORE_FILE_NAME);
        if store_path.exists() {
            fs::remove_file(&store_path)?;
        }
        self.preferred_transcriptions.clear();
        self.replacement_rules.clear();
        self.pending_events.clear();
        Ok(())
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
