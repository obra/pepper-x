mod transcriber;

pub use transcriber::{
    transcribe_wav, TranscriptionError, TranscriptionRequest, TranscriptionResult, BACKEND_NAME,
};
