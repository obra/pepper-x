# Pepper X Improvement Spec: Ghost Pepper Parity

This spec identifies what Pepper X needs to match or exceed Ghost Pepper's capabilities on Linux/GNOME. It covers the cleanup prompt, OCR context, transcription lab, correction learning, sound feedback, and UX polish. Each section states what Ghost Pepper does, what Pepper X has today, and what to build.

---

## 1. Cleanup Prompt Overhaul

### Ghost Pepper behavior

The cleanup prompt is assembled from four structured sections joined by double newlines:

1. **Base prompt** (user custom or built-in default). The built-in default:
   - Deletes filler words: um, uh, like, you know, basically, literally, sort of, kind of
   - Handles self-correction commands: "scratch that", "never mind", "no let me start over" delete preceding content
   - Fixes recognition errors for names/commands/files/jargon when context is clear
   - Properly punctuates sentences
   - Honors explicit punctuation/spelling requests
   - Reproduces the entire transcript -- never deletes sentences or summarizes
   - Output must read as professionally written by a human
   - Includes input/output examples demonstrating filler removal and self-correction
2. **CORRECTION-HINTS block**: Preferred transcriptions as bullet items, misheard replacements as arrow pairs (e.g., `- chat gbt -> ChatGPT`)
3. **OCR-RULES block**: Instructions to treat window text as disambiguation context only -- prefer spoken words, correct likely misrecognitions of visible names/commands/files
4. **WINDOW-OCR-CONTENT block**: Captured screen text, truncated to 4000 chars

The transcript itself is wrapped in `<USER-INPUT>...</USER-INPUT>` tags.

Inference uses temperature 0.1, thinking mode "suppressed" (reasoning tokens generated but excluded from output), and a 15-second timeout.

Output sanitization strips `<think>...</think>` blocks and orphan opening think tags.

### Pepper X today

- Two prompt profiles (ordinary-dictation, literal-dictation) with minimal instructions
- No filler word removal instructions
- No self-correction handling ("scratch that")
- No input/output examples
- No XML block structure (USER-INPUT, CORRECTION-HINTS, OCR-RULES, WINDOW-OCR-CONTENT)
- Correction memory passed as generic text with "Saved correction memory:" label
- OCR context passed with "Optional OCR context:" label, limited to 512 chars
- Greedy sampling (no temperature), no thinking mode
- No output sanitization for reasoning tags
- No inference timeout

### What to build

- [ ] Rewrite the default prompt to match Ghost Pepper's (filler removal, self-correction, examples)
- [ ] Wrap transcript in `<USER-INPUT>...</USER-INPUT>` tags
- [ ] Format corrections into `<CORRECTION-HINTS>` block with bullets and arrow pairs
- [ ] Add `<OCR-RULES>` block when window context is present
- [ ] Wrap OCR text in `<WINDOW-OCR-CONTENT>` block
- [ ] Increase OCR context limit from 512 to 4000 chars
- [ ] Add temperature=0.1 to inference (requires checking llama_cpp crate API)
- [ ] Add output sanitization: strip `<think>...</think>` blocks and orphan think tags
- [ ] Add 15-second inference timeout
- [ ] Implement fallback: if model produces empty/"..." output, fall back to deterministically corrected text

---

## 2. Window OCR Context Improvements

### Ghost Pepper behavior

- Screenshot captured at recording START (prefetch) so result is ready before transcription completes
- Three-state lifecycle: Idle -> Running -> Resolved
- Prefetch cancelled if cleanup disabled or window context disabled
- OCR uses "accurate" recognition level with language correction enabled
- Custom vocabulary from correction store improves recognition
- Text sorted in natural reading order (top-to-bottom, left-to-right)
- Average confidence computed
- Feature gated by user setting (default: off)
- 4000-char limit

### Pepper X today

- Screenshot capture exists via GNOME Shell D-Bus (screenshot.rs)
- Tesseract OCR exists (context.rs)
- AT-SPI text fallback exists
- 512-char limit
- No prefetch at recording start -- OCR runs after transcription
- No user-facing toggle for window context
- No custom vocabulary for OCR

### What to build

- [ ] Start OCR prefetch when recording begins (not after transcription)
- [ ] Add user setting for window context enable/disable (default: off)
- [ ] Increase OCR context limit to 4000 chars
- [ ] Pass correction store terms as custom vocabulary to Tesseract (if supported)
- [ ] Add prefetch lifecycle (idle/running/resolved) with cancellation

---

## 3. Transcription Lab Enhancements

### Ghost Pepper behavior

Full experiment workbench:
- Browser mode: list of up to 50 archived recordings, newest-first, with date/time, transcript preview, audio duration, copy button
- Detail mode: selected recording with original + experiment results
- Transcription rerun: pick a different ASR model, run, see diff vs original
- Cleanup rerun: pick a different cleanup model, edit the prompt inline, toggle OCR on/off, run, see diff vs original
- View full cleanup transcript (prompt + raw model output) for debugging
- Audio playback button for WAV recordings
- Experiment results are ephemeral -- entry data is never modified
- Model selections sync back to app settings

### Pepper X today

- History browser with archived recordings
- Transcription rerun with model selection
- No cleanup rerun
- No inline prompt editor
- No OCR toggle per rerun
- No diff views
- No audio playback
- No cleanup transcript viewer

### What to build

- [ ] Add cleanup rerun: model picker, prompt editor, OCR toggle, run button
- [ ] Add diff view comparing original vs experiment (both transcription and cleanup)
- [ ] Add "Show cleanup transcript" button showing full prompt + raw output
- [ ] Add audio playback for archived WAV files
- [ ] Ensure experiment state resets on entry change
- [ ] Sync model selections back to app settings

---

## 4. Post-Paste Correction Learning

### Ghost Pepper behavior

After text is inserted, the system watches for the user editing the inserted text. If the user corrects a word within a short time window, the system offers to learn the correction as either:
- A preferred transcription (correct capitalization/spelling)
- A misheard replacement (wrong word -> right word)

Learned corrections are applied in future cleanup runs both deterministically and as LLM hints.

### Pepper X today

- `CorrectionStore` exists with preferred transcriptions and replacements
- `learn_correction()` function exists with validation
- Some post-paste learning infrastructure in transcription.rs
- Learning constraints check that corrections are meaningful

### What to build

- [ ] Verify the full post-paste learning flow works end-to-end
- [ ] Add UI for reviewing and managing saved corrections
- [ ] Ensure learned corrections appear in CORRECTION-HINTS block

---

## 5. Sound Effects

### Ghost Pepper behavior

- Start recording: system sound "Tink"
- Stop recording: system sound "Pop"
- Gated by `playSounds` user preference (default: enabled)
- Sound playback is cancellable and re-triggerable

### Pepper X today

- No sound effects at all

### What to build

- [ ] Add start/stop recording sound effects using PipeWire or GStreamer
- [ ] Add user setting for sound enable/disable
- [ ] Select appropriate Linux system sounds (or bundle short audio files)

---

## 6. Menu Bar / System Tray Polish

### Ghost Pepper behavior

Menu bar icon states:
- **Recording**: red-tinted icon
- **Loading**: orange ellipsis
- **Error**: yellow warning triangle
- **Ready/Transcribing/Cleaning up**: template icon (adapts to dark/light)

Dropdown menu shows:
- Settings button
- Debug Log button
- Version label
- Dynamic status line (Recording.../Transcribing.../Cleaning up...)
- Progress bar during model loading
- Error message with retry button when applicable

### Pepper X today

- GNOME Shell extension provides tray indicator
- Status polling via D-Bus (GetLiveStatus)
- GetCapabilities reports feature support
- Icon state changes exist but limited

### What to build

- [ ] Add distinct icon states for recording/loading/error/ready
- [ ] Add version label to extension dropdown
- [ ] Add model loading progress bar to extension
- [ ] Add error message with retry action to extension dropdown

---

## 7. Recording Pipeline Polish

### Ghost Pepper behavior

- 200ms flush delay after stop to capture trailing audio
- Performance trace from hotkey press to insertion complete
- Pre-warm audio engine on startup to reduce first-recording latency
- Audio serialized as 16-bit PCM WAV (16kHz mono)

### Pepper X today

- Recording via PipeWire exists
- WAV serialization exists
- No explicit flush delay
- No pre-warm
- No performance trace

### What to build

- [ ] Add 200ms flush delay after recording stop
- [ ] Add audio engine pre-warm on startup
- [ ] Add performance trace (timestamps from hotkey press through insertion)

---

## 8. Settings UX Completeness

### Ghost Pepper settings surface

- Microphone picker with live level meter
- Hotkey configuration (push-to-talk, toggle-to-talk, key selection)
- Sound effects toggle
- Cleanup enable/disable
- Cleanup model picker (0.8B/2B/4B)
- Speech model picker
- Prompt profile selector
- Custom prompt editor
- Window context toggle
- Correction memory viewer/editor
- Launch at login toggle
- Auto-update toggle (N/A on Linux)

### Pepper X today

- Microphone picker with on-demand level check
- Cleanup enable/disable
- Cleanup model picker
- Transcription model picker
- Prompt profile selector
- Custom prompt editor
- Launch at login toggle
- No hotkey configuration UI
- No sound effects toggle
- No window context toggle
- No correction memory viewer

### What to build

- [ ] Add hotkey configuration (modifier key selection)
- [ ] Add sound effects toggle
- [ ] Add window context toggle
- [ ] Add correction memory viewer/editor
- [ ] Consider: trigger mode selector (hold-to-talk vs toggle-to-talk)

---

## Priority Order

**P0 -- Cleanup quality (biggest user-visible impact):**
1. Cleanup prompt overhaul (Section 1)
2. OCR prefetch and context improvements (Section 2)

**P1 -- Lab and learning:**
3. Transcription lab cleanup rerun + diff views (Section 3)
4. Post-paste correction learning verification (Section 4)

**P2 -- Polish:**
5. Sound effects (Section 5)
6. Menu bar icon states (Section 6)
7. Recording pipeline polish (Section 7)
8. Settings completeness (Section 8)
