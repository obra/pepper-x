pub mod cleanup;

pub use cleanup::{
    cleanup_prompt, cleanup_system_prompt, prefill_cleanup_system_prompt, run_cleanup,
    CleanupError, CleanupRequest, CleanupResult, LITERAL_DICTATION_PROMPT_PROFILE,
    ORDINARY_DICTATION_PROMPT_PROFILE,
};

#[cfg(test)]
mod cleanup_runtime {
    use super::{cleanup_prompt, run_cleanup, CleanupError, CleanupRequest};
    use crate::cleanup::{strip_reasoning_tags, ORDINARY_DICTATION_PROMPT_PROFILE};
    use std::path::PathBuf;

    #[test]
    fn cleanup_runtime_rejects_missing_model_path() {
        let error = run_cleanup(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-missing.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        })
        .unwrap_err();

        assert_eq!(
            error,
            CleanupError::MissingModelPath(PathBuf::from("/tmp/pepper-x-missing.gguf"))
        );
    }

    #[test]
    fn cleanup_runtime_prompt_is_deterministic_for_same_transcript_input() {
        let request = CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        };

        assert_eq!(cleanup_prompt(&request), cleanup_prompt(&request));
    }

    #[test]
    fn cleanup_runtime_prompt_profile_changes_the_prompt_contract() {
        let ordinary_prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });
        let literal_prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: "literal-dictation".into(),
            custom_prompt_text: None,
        });

        assert_ne!(ordinary_prompt, literal_prompt);
        assert!(literal_prompt.contains("Preserve spoken filler words"));
    }

    #[test]
    fn cleanup_prompt_wraps_transcript_in_user_input_tags() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        assert!(prompt.contains("<USER-INPUT>\nhello from pepper x\n</USER-INPUT>"));
    }

    #[test]
    fn cleanup_prompt_includes_filler_word_rules() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "test".into(),
            model_path: PathBuf::from("/tmp/model.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        assert!(prompt.contains("Delete fillers"));
        assert!(prompt.contains("scratch that"));
        assert!(prompt.contains("never mind"));
    }

    #[test]
    fn cleanup_prompt_includes_examples() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "test".into(),
            model_path: PathBuf::from("/tmp/model.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        assert!(prompt.contains("Example:"));
        assert!(prompt.contains("->"));
    }

    #[test]
    fn cleanup_ocr_prompt_omits_context_when_absent() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        assert!(!prompt.contains("<OCR-RULES>"));
        assert!(!prompt.contains("<WINDOW-OCR-CONTENT>"));
    }

    #[test]
    fn cleanup_ocr_prompt_uses_xml_blocks_when_present() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: Some("Terminal: ~/git/pepper-x".into()),
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        assert!(prompt.contains("<OCR-RULES>"));
        assert!(prompt.contains("</OCR-RULES>"));
        assert!(prompt.contains("<WINDOW-OCR-CONTENT>\nTerminal: ~/git/pepper-x\n</WINDOW-OCR-CONTENT>"));
    }

    #[test]
    fn cleanup_ocr_prompt_bounds_context_to_4000_chars() {
        let oversized_ocr = "A".repeat(5000);
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: Some(oversized_ocr.clone()),
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        let bounded_ocr = "A".repeat(4000);
        assert!(prompt.contains(&bounded_ocr));
        assert!(!prompt.contains(&oversized_ocr));
    }

    #[test]
    fn cleanup_ocr_prompt_includes_supporting_context_in_ocr_block() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: Some("line before\nline after".into()),
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        assert!(prompt.contains("<WINDOW-OCR-CONTENT>\nline before\nline after\n</WINDOW-OCR-CONTENT>"));
        assert!(prompt.contains("<OCR-RULES>"));
    }

    #[test]
    fn cleanup_runtime_prompt_includes_correction_hints_block() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: Some(
                "- pepper x -> Pepper X\n- chat gbt -> ChatGPT".into(),
            ),
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        });

        assert!(prompt.contains("<CORRECTION-HINTS>"));
        assert!(prompt.contains("pepper x -> Pepper X"));
        assert!(prompt.contains("</CORRECTION-HINTS>"));
    }

    #[test]
    #[ignore = "requires a real cleanup model"]
    fn cleanup_real_runs_against_a_real_model() {
        let model_path = std::env::var_os("PEPPERX_CLEANUP_MODEL_PATH")
            .map(PathBuf::from)
            .expect("PEPPERX_CLEANUP_MODEL_PATH must point to a GGUF cleanup model");

        let result = run_cleanup(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path,
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
        })
        .expect("real cleanup run should succeed");

        assert!(!result.cleaned_text.trim().is_empty());
    }

    #[test]
    fn cleanup_runtime_custom_prompt_layers_on_top_of_selected_profile() {
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: Some("Return SHOUTING ONLY.".into()),
        });

        assert!(prompt.contains("Delete fillers"));
        assert!(prompt.contains("Return SHOUTING ONLY.\n"));
    }

    #[test]
    fn cleanup_runtime_custom_prompt_preserves_user_whitespace() {
        let custom_prompt = "\n  Keep product names verbatim.\n\nDo not normalize punctuation.\n";
        let prompt = cleanup_prompt(&CleanupRequest {
            transcript_text: "hello from pepper x".into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: Some(custom_prompt.into()),
        });

        assert!(prompt.contains(custom_prompt));
    }

    #[test]
    fn strip_reasoning_tags_removes_matched_think_blocks() {
        let input = "before <think>internal reasoning</think> after";
        assert_eq!(strip_reasoning_tags(input), "before  after");
    }

    #[test]
    fn strip_reasoning_tags_removes_orphan_leading_think_tag() {
        let input = "<think>orphan reasoning without closing\nThe actual output.";
        assert_eq!(strip_reasoning_tags(input), "The actual output.");
    }

    #[test]
    fn strip_reasoning_tags_preserves_text_without_think_tags() {
        let input = "Hello from Pepper X.";
        assert_eq!(strip_reasoning_tags(input), "Hello from Pepper X.");
    }

    #[test]
    fn strip_reasoning_tags_handles_think_with_attributes() {
        let input = "<think type=\"internal\">reasoning</think>Clean output.";
        assert_eq!(strip_reasoning_tags(input), "Clean output.");
    }
}
