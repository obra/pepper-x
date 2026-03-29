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
    fn correction_store_formats_preferred_transcriptions_as_prompt_memory() {
        let mut store = CorrectionStore::new(temp_root("preferred-override"));
        store.set_preferred_transcription("um i guess", "I guess");
        store.add_replacement_rule("guess", "think");

        let prompt_memory = store
            .prompt_memory_text()
            .expect("prompt memory should be present");

        assert!(prompt_memory.contains("Preferred transcript examples:"));
        assert!(prompt_memory.contains("- raw: um i guess"));
        assert!(prompt_memory.contains("  preferred: I guess"));
    }

    #[test]
    fn correction_store_formats_replacement_rules_as_prompt_memory() {
        let mut store = CorrectionStore::new(temp_root("deterministic-replacements"));
        store.add_replacement_rule("pepper", "Pepper");
        store.add_replacement_rule("pepper x", "Pepper X");

        let prompt_memory = store
            .prompt_memory_text()
            .expect("prompt memory should be present");

        assert!(prompt_memory.contains("Preferred replacements:"));
        assert!(prompt_memory.contains("- pepper x => Pepper X"));
        assert!(prompt_memory.contains("- pepper => Pepper"));
    }

    #[test]
    fn correction_persists_and_reloads_store() {
        let root = temp_root("persist-reload");
        let mut store = CorrectionStore::new(root.clone());
        store.set_preferred_transcription("uh turn on mic", "Turn on the mic");
        store.add_replacement_rule("mic", "microphone");
        store.persist().unwrap();

        let reloaded = CorrectionStore::load(root).unwrap();

        let prompt_memory = reloaded
            .prompt_memory_text()
            .expect("prompt memory should be present");

        assert!(prompt_memory.contains("- raw: uh turn on mic"));
        assert!(prompt_memory.contains("  preferred: Turn on the mic"));
        assert!(prompt_memory.contains("- mic => microphone"));
    }

    #[test]
    fn correction_store_appends_auditable_history_entries() {
        let root = temp_root("append-only-history");
        let mut store = CorrectionStore::new(root.clone());
        store.set_preferred_transcription("one", "One");
        store.persist().unwrap();

        store.add_replacement_rule("two", "Two");
        store.persist().unwrap();

        let store_path = root.join("corrections.jsonl");
        let contents = std::fs::read_to_string(&store_path).unwrap();
        let entries: Vec<_> = contents.lines().collect();

        assert_eq!(entries.len(), 2);
        assert!(entries[0].contains("\"preferred_transcription\""));
        assert!(entries[1].contains("\"replacement_rule\""));

        let reloaded = CorrectionStore::load(root).unwrap();
        let prompt_memory = reloaded
            .prompt_memory_text()
            .expect("prompt memory should be present");

        assert!(prompt_memory.contains("- raw: one"));
        assert!(prompt_memory.contains("- two => Two"));
    }

    #[test]
    fn learning_accepts_successful_inserted_phrase_normalization() {
        let learned = learn_correction("hello from pepper x", "Hello from Pepper X.", true);

        assert_eq!(
            learned,
            Some(LearnedCorrection {
                source: "hello from pepper x".into(),
                replacement: "Hello from Pepper X.".into(),
            })
        );
    }

    #[test]
    fn learning_rejects_failed_insertions() {
        let learned = learn_correction("hello from pepper x", "Hello from Pepper X.", false);

        assert_eq!(learned, None);
    }

    #[test]
    fn learning_rejects_low_confidence_or_destructive_updates() {
        let low_confidence = learn_correction("pepper x", "paper x", true);
        let destructive = learn_correction("hello from pepper x", "Hello", true);

        assert_eq!(low_confidence, None);
        assert_eq!(destructive, None);
    }
}
