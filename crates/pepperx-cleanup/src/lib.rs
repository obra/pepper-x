pub mod cleanup;

pub use cleanup::{
    cleanup_prompt, cleanup_system_prompt, prefill_cleanup_system_prompt, run_cleanup,
    CleanupError, CleanupRequest, CleanupResult, CHAT_TEMPLATE_CHATML, CHAT_TEMPLATE_GEMMA,
    LITERAL_DICTATION_PROMPT_PROFILE, ORDINARY_DICTATION_PROMPT_PROFILE,
};

#[cfg(test)]
mod cleanup_runtime {
    use super::{cleanup_prompt, run_cleanup, CleanupError, CleanupRequest};
    use crate::cleanup::{
        strip_reasoning_tags, CHAT_TEMPLATE_CHATML, CHAT_TEMPLATE_GEMMA,
        ORDINARY_DICTATION_PROMPT_PROFILE,
    };
    use std::path::PathBuf;

    fn chatml_request(transcript: &str) -> CleanupRequest {
        CleanupRequest {
            transcript_text: transcript.into(),
            model_path: PathBuf::from("/tmp/pepper-x-present.gguf"),
            supporting_context_text: None,
            ocr_text: None,
            correction_memory_text: None,
            prompt_profile: ORDINARY_DICTATION_PROMPT_PROFILE.into(),
            custom_prompt_text: None,
            chat_template: CHAT_TEMPLATE_CHATML.into(),
        }
    }

    fn gemma_request(transcript: &str) -> CleanupRequest {
        CleanupRequest {
            chat_template: CHAT_TEMPLATE_GEMMA.into(),
            ..chatml_request(transcript)
        }
    }

    #[test]
    fn cleanup_runtime_rejects_missing_model_path() {
        let error = run_cleanup(&CleanupRequest {
            model_path: PathBuf::from("/tmp/pepper-x-missing.gguf"),
            ..chatml_request("hello from pepper x")
        })
        .unwrap_err();

        assert_eq!(
            error,
            CleanupError::MissingModelPath(PathBuf::from("/tmp/pepper-x-missing.gguf"))
        );
    }

    #[test]
    fn cleanup_runtime_prompt_is_deterministic_for_same_transcript_input() {
        let request = chatml_request("hello from pepper x");
        assert_eq!(cleanup_prompt(&request), cleanup_prompt(&request));
    }

    #[test]
    fn cleanup_runtime_prompt_profile_changes_the_prompt_contract() {
        let ordinary_prompt = cleanup_prompt(&chatml_request("hello from pepper x"));
        let literal_prompt = cleanup_prompt(&CleanupRequest {
            prompt_profile: "literal-dictation".into(),
            ..chatml_request("hello from pepper x")
        });

        assert_ne!(ordinary_prompt, literal_prompt);
        assert!(literal_prompt.contains("Preserve spoken filler words"));
    }

    #[test]
    fn cleanup_prompt_wraps_transcript_in_user_input_tags() {
        let prompt = cleanup_prompt(&chatml_request("hello from pepper x"));
        assert!(prompt.contains("<USER-INPUT>\nhello from pepper x\n</USER-INPUT>"));
    }

    #[test]
    fn cleanup_prompt_includes_filler_word_rules() {
        let prompt = cleanup_prompt(&chatml_request("test"));
        assert!(prompt.contains("Remove filler words"));
        assert!(prompt.contains("scratch that"));
        assert!(prompt.contains("never mind"));
    }

    #[test]
    fn cleanup_prompt_includes_examples() {
        let prompt = cleanup_prompt(&chatml_request("test"));
        assert!(prompt.contains("scratch that"));
        assert!(prompt.contains("never mind"));
    }

    #[test]
    fn cleanup_ocr_prompt_omits_context_when_absent() {
        let prompt = cleanup_prompt(&chatml_request("hello from pepper x"));
        assert!(!prompt.contains("<OCR-RULES>"));
        assert!(!prompt.contains("<WINDOW-OCR-CONTENT>"));
    }

    #[test]
    fn cleanup_ocr_prompt_uses_xml_blocks_when_present() {
        let prompt = cleanup_prompt(&CleanupRequest {
            ocr_text: Some("Terminal: ~/git/pepper-x".into()),
            ..chatml_request("hello from pepper x")
        });

        assert!(prompt.contains("<OCR-RULES>"));
        assert!(prompt.contains("</OCR-RULES>"));
        assert!(prompt.contains("<WINDOW-OCR-CONTENT>\nTerminal: ~/git/pepper-x\n</WINDOW-OCR-CONTENT>"));
    }

    #[test]
    fn cleanup_ocr_prompt_bounds_context_to_4000_chars() {
        let oversized_ocr = "A".repeat(5000);
        let prompt = cleanup_prompt(&CleanupRequest {
            ocr_text: Some(oversized_ocr.clone()),
            ..chatml_request("hello from pepper x")
        });

        let bounded_ocr = "A".repeat(4000);
        assert!(prompt.contains(&bounded_ocr));
        assert!(!prompt.contains(&oversized_ocr));
    }

    #[test]
    fn cleanup_ocr_prompt_includes_supporting_context_in_ocr_block() {
        let prompt = cleanup_prompt(&CleanupRequest {
            supporting_context_text: Some("line before\nline after".into()),
            ..chatml_request("hello from pepper x")
        });

        assert!(prompt.contains("<WINDOW-OCR-CONTENT>\nline before\nline after\n</WINDOW-OCR-CONTENT>"));
        assert!(prompt.contains("<OCR-RULES>"));
    }

    #[test]
    fn cleanup_runtime_prompt_includes_correction_hints_block() {
        let prompt = cleanup_prompt(&CleanupRequest {
            correction_memory_text: Some(
                "- pepper x -> Pepper X\n- chat gbt -> ChatGPT".into(),
            ),
            ..chatml_request("hello from pepper x")
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
            model_path,
            ..chatml_request("hello from pepper x")
        })
        .expect("real cleanup run should succeed");

        assert!(!result.cleaned_text.trim().is_empty());
    }

    #[test]
    fn cleanup_runtime_custom_prompt_layers_on_top_of_selected_profile() {
        let prompt = cleanup_prompt(&CleanupRequest {
            custom_prompt_text: Some("Return SHOUTING ONLY.".into()),
            ..chatml_request("hello from pepper x")
        });

        assert!(prompt.contains("Remove filler words"));
        assert!(prompt.contains("Return SHOUTING ONLY.\n"));
    }

    #[test]
    fn cleanup_runtime_custom_prompt_preserves_user_whitespace() {
        let custom_prompt = "\n  Keep product names verbatim.\n\nDo not normalize punctuation.\n";
        let prompt = cleanup_prompt(&CleanupRequest {
            custom_prompt_text: Some(custom_prompt.into()),
            ..chatml_request("hello from pepper x")
        });

        assert!(prompt.contains(custom_prompt));
    }

    // --- ChatML-specific tests ---

    #[test]
    fn chatml_prompt_uses_im_start_im_end_tokens() {
        let prompt = cleanup_prompt(&chatml_request("test"));
        assert!(prompt.contains("<|im_start|>system\n"));
        assert!(prompt.contains("<|im_end|>"));
        assert!(prompt.contains("<|im_start|>assistant\n"));
    }

    #[test]
    fn chatml_prompt_includes_no_think_directive() {
        let prompt = cleanup_prompt(&chatml_request("test"));
        assert!(prompt.contains("/no_think"));
    }

    // --- Gemma template tests ---

    #[test]
    fn gemma_prompt_uses_start_of_turn_tokens() {
        let prompt = cleanup_prompt(&gemma_request("test"));
        assert!(prompt.contains("<start_of_turn>user\n"));
        assert!(prompt.contains("<end_of_turn>"));
        assert!(prompt.contains("<start_of_turn>model\n"));
    }

    #[test]
    fn gemma_prompt_does_not_contain_chatml_tokens() {
        let prompt = cleanup_prompt(&gemma_request("test"));
        assert!(!prompt.contains("<|im_start|>"));
        assert!(!prompt.contains("<|im_end|>"));
    }

    #[test]
    fn gemma_prompt_omits_no_think_directive() {
        let prompt = cleanup_prompt(&gemma_request("test"));
        assert!(!prompt.contains("/no_think"));
    }

    #[test]
    fn gemma_prompt_includes_cleanup_rules() {
        let prompt = cleanup_prompt(&gemma_request("test"));
        assert!(prompt.contains("Remove filler words"));
        assert!(prompt.contains("scratch that"));
    }

    #[test]
    fn gemma_prompt_wraps_transcript_in_user_input_tags() {
        let prompt = cleanup_prompt(&gemma_request("hello from pepper x"));
        assert!(prompt.contains("<USER-INPUT>\nhello from pepper x\n</USER-INPUT>"));
    }

    // --- strip_reasoning_tags tests ---

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
