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
        let data: CorrectionStoreData =
            serde_json::from_str(&data).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

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

    pub fn apply(&self, input: &str) -> String {
        if let Some(preferred) = self.preferred_transcriptions.get(input) {
            return preferred.clone();
        }

        let mut rules: Vec<_> = self.replacement_rules.iter().collect();
        rules.sort_by(|left, right| {
            right
                .0
                .len()
                .cmp(&left.0.len())
                .then_with(|| left.0.cmp(right.0))
        });

        let mut output = String::with_capacity(input.len());
        let mut index = 0;

        while index < input.len() {
            let mut matched_rule = None;

            for (source, replacement) in &rules {
                if input[index..].starts_with(source.as_str()) {
                    matched_rule = Some((source.as_str(), replacement.as_str()));
                    output.push_str(replacement);
                    index += source.len();
                    break;
                }
            }

            if matched_rule.is_none() {
                let current = input[index..].chars().next().expect("valid utf-8 boundary");
                output.push(current);
                index += current.len_utf8();
            }
        }

        output
    }

    pub fn persist(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let store_path = self.root.join(STORE_FILE_NAME);
        let data = CorrectionStoreData {
            preferred_transcriptions: self.preferred_transcriptions.clone(),
            replacement_rules: self.replacement_rules.clone(),
        };
        let json = serde_json::to_string_pretty(&data).expect("correction store data should serialize");

        fs::write(store_path, json)
    }
}
