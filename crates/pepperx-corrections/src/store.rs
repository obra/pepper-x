use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

const STORE_FILE_NAME: &str = "corrections.json";

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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CorrectionStoreData {
    preferred_transcriptions: BTreeMap<String, String>,
    replacement_rules: BTreeMap<String, String>,
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
        let data: CorrectionStoreData = serde_json::from_str(&data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        Ok(Self {
            root,
            preferred_transcriptions: data.preferred_transcriptions,
            replacement_rules: data.replacement_rules,
        })
    }

    pub fn set_preferred_transcription(
        &mut self,
        source: impl Into<String>,
        replacement: impl Into<String>,
    ) {
        self.preferred_transcriptions
            .insert(source.into(), replacement.into());
    }

    pub fn add_replacement_rule(
        &mut self,
        source: impl Into<String>,
        replacement: impl Into<String>,
    ) {
        self.replacement_rules
            .insert(source.into(), replacement.into());
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

    pub fn persist(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let store_path = self.root.join(STORE_FILE_NAME);
        let data = CorrectionStoreData {
            preferred_transcriptions: self.preferred_transcriptions.clone(),
            replacement_rules: self.replacement_rules.clone(),
        };
        let json =
            serde_json::to_string_pretty(&data).expect("correction store data should serialize");

        fs::write(store_path, json)
    }
}
