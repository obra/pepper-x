mod learning;
mod store;

pub use learning::{learn_correction, LearnedCorrection};
pub use store::{CorrectionStore, PreferredTranscription, ReplacementRule};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pepper-x-corrections-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn correction_applies_exact_preferred_transcription_override() {
        let mut store = CorrectionStore::new(temp_root("preferred-override"));
        store.set_preferred_transcription("um i guess", "I guess");
        store.add_replacement_rule("guess", "think");

        let corrected = store.apply("um i guess");

        assert_eq!(corrected, "I guess");
    }

    #[test]
    fn correction_applies_deterministic_replacements_after_cleanup() {
        let mut store = CorrectionStore::new(temp_root("deterministic-replacements"));
        store.add_replacement_rule("pepper", "Pepper");
        store.add_replacement_rule("pepper x", "Pepper X");

        let corrected = store.apply("pepper x and pepper");

        assert_eq!(corrected, "Pepper X and Pepper");
    }

    #[test]
    fn correction_persists_and_reloads_store() {
        let root = temp_root("persist-reload");
        let mut store = CorrectionStore::new(root.clone());
        store.set_preferred_transcription("uh turn on mic", "Turn on the mic");
        store.add_replacement_rule("mic", "microphone");
        store.persist().unwrap();

        let reloaded = CorrectionStore::load(root).unwrap();

        assert_eq!(reloaded.apply("uh turn on mic"), "Turn on the mic");
        assert_eq!(reloaded.apply("the mic is live"), "the microphone is live");
    }
}
