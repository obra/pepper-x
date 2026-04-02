use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use bzip2::read::BzDecoder;
use tar::Archive;

use crate::cache::{model_install_dir, model_readiness, ModelInventoryEntry, ModelReadiness};
use crate::catalog::{
    default_model, CatalogModel, DownloadArtifact, DownloadArtifactKind, ModelKind,
};

const DOWNLOADS_DIR_NAME: &str = ".downloads";
const PARTIAL_INSTALL_SUFFIX: &str = ".partial";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSupport {
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapModelPhase {
    Pending,
    Downloading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapModelState {
    pub model_id: String,
    pub kind: ModelKind,
    pub phase: BootstrapModelPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapProgress {
    pub total_models: usize,
    pub completed_models: usize,
    pub current_model_id: Option<String>,
    pub failure_message: Option<String>,
    pub model_states: Vec<BootstrapModelState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapError {
    Io(String),
    Fetch {
        url: String,
        target_path: PathBuf,
        message: String,
    },
    ExtractArchive {
        archive_path: PathBuf,
        message: String,
    },
    InvalidArchiveEntry(PathBuf),
    IncompleteInstall {
        model_id: String,
        install_path: PathBuf,
        missing_files: Vec<String>,
    },
}

impl std::error::Error for BootstrapError {}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => f.write_str(message),
            Self::Fetch {
                url,
                target_path,
                message,
            } => write!(
                f,
                "failed to fetch Pepper X model artifact {url} into {}: {message}",
                target_path.display()
            ),
            Self::ExtractArchive {
                archive_path,
                message,
            } => write!(
                f,
                "failed to extract Pepper X model archive {}: {message}",
                archive_path.display()
            ),
            Self::InvalidArchiveEntry(entry_path) => write!(
                f,
                "Pepper X model archive contains an unsafe path: {}",
                entry_path.display()
            ),
            Self::IncompleteInstall {
                model_id,
                install_path,
                missing_files,
            } => write!(
                f,
                "Pepper X model {model_id} is incomplete at {}: missing {}",
                install_path.display(),
                missing_files.join(", ")
            ),
        }
    }
}

pub fn download_support() -> DownloadSupport {
    DownloadSupport { supported: true }
}

pub fn model_inventory(cache_root: &Path) -> Vec<ModelInventoryEntry> {
    crate::cache::model_inventory(cache_root)
}

pub fn bootstrap_model(
    model: &CatalogModel,
    cache_root: &Path,
) -> Result<ModelReadiness, BootstrapError> {
    bootstrap_model_with_fetch(model, cache_root, |url, target_path| {
        download_to_path(url, target_path)
    })
}

pub fn bootstrap_default_models_with_progress<P>(
    cache_root: &Path,
    progress: P,
) -> Result<BootstrapProgress, BootstrapError>
where
    P: FnMut(&BootstrapProgress),
{
    bootstrap_default_models_with_fetch(cache_root, progress, |url, target_path| {
        download_to_path(url, target_path)
    })
}

pub fn bootstrap_default_models_with_fetch<P, F, E>(
    cache_root: &Path,
    mut progress: P,
    mut fetch: F,
) -> Result<BootstrapProgress, BootstrapError>
where
    P: FnMut(&BootstrapProgress),
    F: FnMut(&str, &Path) -> Result<(), E>,
    E: fmt::Display,
{
    let models = [
        default_model(ModelKind::Asr),
        default_model(ModelKind::Cleanup),
    ];
    let mut snapshot = bootstrap_progress_snapshot(&models, cache_root);
    progress(&snapshot);

    for model in models {
        let Some(index) = snapshot
            .model_states
            .iter()
            .position(|state| state.model_id == model.id)
        else {
            continue;
        };

        if snapshot.model_states[index].phase == BootstrapModelPhase::Ready {
            continue;
        }

        snapshot.current_model_id = Some(model.id.into());
        snapshot.failure_message = None;
        snapshot.model_states[index].phase = BootstrapModelPhase::Downloading;
        progress(&snapshot);

        match bootstrap_model_with_fetch(model, cache_root, |url, target_path| {
            fetch(url, target_path)
        }) {
            Ok(_) => {
                snapshot.model_states[index].phase = BootstrapModelPhase::Ready;
                snapshot.completed_models = snapshot
                    .model_states
                    .iter()
                    .filter(|state| state.phase == BootstrapModelPhase::Ready)
                    .count();
                snapshot.current_model_id = None;
                progress(&snapshot);
            }
            Err(error) => {
                snapshot.model_states[index].phase = BootstrapModelPhase::Failed;
                snapshot.failure_message = Some(error.to_string());
                snapshot.current_model_id = None;
                progress(&snapshot);
                return Err(error);
            }
        }
    }

    Ok(snapshot)
}

pub fn bootstrap_model_with_fetch<F, E>(
    model: &CatalogModel,
    cache_root: &Path,
    mut fetch: F,
) -> Result<ModelReadiness, BootstrapError>
where
    F: FnMut(&str, &Path) -> Result<(), E>,
    E: fmt::Display,
{
    let readiness = model_readiness(model, cache_root);
    if readiness.is_ready {
        return Ok(readiness);
    }

    match model.download_artifact.kind {
        DownloadArtifactKind::File => bootstrap_file(model, cache_root, &mut fetch)?,
        DownloadArtifactKind::TarBz2 => bootstrap_tar_bz2(model, cache_root, &mut fetch)?,
        DownloadArtifactKind::MultiFile => {
            bootstrap_multi_file(model, cache_root, &mut fetch)?
        }
    }

    let readiness = model_readiness(model, cache_root);
    if readiness.is_ready {
        Ok(readiness)
    } else {
        Err(BootstrapError::IncompleteInstall {
            model_id: model.id.into(),
            install_path: readiness.install_path.clone(),
            missing_files: readiness.missing_files,
        })
    }
}

fn bootstrap_file<F, E>(
    model: &CatalogModel,
    cache_root: &Path,
    fetch: &mut F,
) -> Result<(), BootstrapError>
where
    F: FnMut(&str, &Path) -> Result<(), E>,
    E: fmt::Display,
{
    let install_path = model_install_dir(model, cache_root);
    let temp_path = partial_path(&install_path);
    if let Some(parent) = install_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let _ = fs::remove_file(&temp_path);

    fetch_artifact(&model.download_artifact, &temp_path, fetch)?;
    if install_path.is_file() {
        fs::remove_file(&install_path).map_err(io_error)?;
    }
    fs::rename(&temp_path, &install_path).map_err(io_error)
}

fn bootstrap_multi_file<F, E>(
    model: &CatalogModel,
    cache_root: &Path,
    fetch: &mut F,
) -> Result<(), BootstrapError>
where
    F: FnMut(&str, &Path) -> Result<(), E>,
    E: fmt::Display,
{
    let install_path = model_install_dir(model, cache_root);
    let partial_install_path = partial_path(&install_path);

    let _ = fs::remove_dir_all(&partial_install_path);
    fs::create_dir_all(&partial_install_path).map_err(io_error)?;

    for &(file_name, url) in model.download_files {
        let target = partial_install_path.join(file_name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        fetch(url, &target).map_err(|error| BootstrapError::Fetch {
            url: url.into(),
            target_path: target.clone(),
            message: error.to_string(),
        })?;
    }

    if install_path.is_dir() {
        fs::remove_dir_all(&install_path).map_err(io_error)?;
    }
    if let Some(parent) = install_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    fs::rename(&partial_install_path, &install_path).map_err(io_error)
}

fn bootstrap_tar_bz2<F, E>(
    model: &CatalogModel,
    cache_root: &Path,
    fetch: &mut F,
) -> Result<(), BootstrapError>
where
    F: FnMut(&str, &Path) -> Result<(), E>,
    E: fmt::Display,
{
    let install_path = model_install_dir(model, cache_root);
    let partial_install_path = partial_path(&install_path);
    let downloads_root = cache_root.join(DOWNLOADS_DIR_NAME);
    let archive_path = downloads_root.join(model.download_artifact.file_name);

    fs::create_dir_all(&downloads_root).map_err(io_error)?;
    let _ = fs::remove_dir_all(&partial_install_path);
    let _ = fs::remove_file(&archive_path);
    fetch_artifact(&model.download_artifact, &archive_path, fetch)?;
    fs::create_dir_all(&partial_install_path).map_err(io_error)?;
    extract_tar_bz2(
        &archive_path,
        &partial_install_path,
        model.download_artifact.strip_prefix,
    )?;

    if install_path.is_dir() {
        fs::remove_dir_all(&install_path).map_err(io_error)?;
    }
    if let Some(parent) = install_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    fs::rename(&partial_install_path, &install_path).map_err(io_error)
}

fn fetch_artifact<F, E>(
    artifact: &DownloadArtifact,
    target_path: &Path,
    fetch: &mut F,
) -> Result<(), BootstrapError>
where
    F: FnMut(&str, &Path) -> Result<(), E>,
    E: fmt::Display,
{
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }

    fetch(artifact.url, target_path).map_err(|error| BootstrapError::Fetch {
        url: artifact.url.into(),
        target_path: target_path.to_path_buf(),
        message: error.to_string(),
    })
}

fn bootstrap_progress_snapshot(models: &[&CatalogModel], cache_root: &Path) -> BootstrapProgress {
    let model_states = models
        .iter()
        .map(|model| {
            let readiness = model_readiness(model, cache_root);
            BootstrapModelState {
                model_id: model.id.into(),
                kind: model.kind,
                phase: if readiness.is_ready {
                    BootstrapModelPhase::Ready
                } else {
                    BootstrapModelPhase::Pending
                },
            }
        })
        .collect::<Vec<_>>();

    BootstrapProgress {
        total_models: model_states.len(),
        completed_models: model_states
            .iter()
            .filter(|state| state.phase == BootstrapModelPhase::Ready)
            .count(),
        current_model_id: None,
        failure_message: None,
        model_states,
    }
}

fn extract_tar_bz2(
    archive_path: &Path,
    install_root: &Path,
    strip_prefix: Option<&str>,
) -> Result<(), BootstrapError> {
    let archive = File::open(archive_path).map_err(|error| BootstrapError::ExtractArchive {
        archive_path: archive_path.to_path_buf(),
        message: error.to_string(),
    })?;
    let decoder = BzDecoder::new(archive);
    let mut tar = Archive::new(decoder);

    for entry in tar
        .entries()
        .map_err(|error| BootstrapError::ExtractArchive {
            archive_path: archive_path.to_path_buf(),
            message: error.to_string(),
        })?
    {
        let mut entry = entry.map_err(|error| BootstrapError::ExtractArchive {
            archive_path: archive_path.to_path_buf(),
            message: error.to_string(),
        })?;
        let entry_path = entry
            .path()
            .map_err(|error| BootstrapError::ExtractArchive {
                archive_path: archive_path.to_path_buf(),
                message: error.to_string(),
            })?
            .to_path_buf();
        let relative_path = match strip_prefix {
            Some(prefix) => match entry_path.strip_prefix(prefix) {
                Ok(path) if !path.as_os_str().is_empty() => path.to_path_buf(),
                _ => continue,
            },
            None => entry_path.clone(),
        };
        ensure_safe_relative_path(&relative_path)?;
        extract_archive_entry(&mut entry, install_root, &relative_path)?;
    }

    Ok(())
}

fn extract_archive_entry<R: io::Read>(
    entry: &mut tar::Entry<'_, R>,
    install_root: &Path,
    relative_path: &Path,
) -> Result<(), BootstrapError> {
    let entry_type = entry.header().entry_type();
    if entry_type.is_symlink() || entry_type.is_hard_link() {
        return Err(BootstrapError::InvalidArchiveEntry(
            relative_path.to_path_buf(),
        ));
    }

    let destination = install_root.join(relative_path);
    if entry_type.is_dir() {
        return fs::create_dir_all(&destination).map_err(io_error);
    }

    if !entry_type.is_file() {
        return Err(BootstrapError::InvalidArchiveEntry(
            relative_path.to_path_buf(),
        ));
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }

    let mut output = File::create(&destination).map_err(io_error)?;
    io::copy(entry, &mut output).map_err(io_error)?;
    output.flush().map_err(io_error)
}

fn ensure_safe_relative_path(path: &Path) -> Result<(), BootstrapError> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return Err(BootstrapError::InvalidArchiveEntry(path.to_path_buf()));
    }

    Ok(())
}

fn partial_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("pepper-x-model"));
    path.with_file_name(format!("{file_name}{PARTIAL_INSTALL_SUFFIX}"))
}

fn io_error(error: io::Error) -> BootstrapError {
    BootstrapError::Io(error.to_string())
}

fn download_to_path(url: &str, target_path: &Path) -> Result<(), io::Error> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let response = ureq::get(url)
        .call()
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
    let mut reader = response.into_reader();
    let mut file = File::create(target_path)?;
    io::copy(&mut reader, &mut file)?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{catalog_model, model_readiness};
    use bzip2::write::BzEncoder;
    use std::cell::RefCell;
    use std::io::Write;
    use std::rc::Rc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tar::Builder;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pepper-x-model-bootstrap-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn download_bootstraps_nemotron_asr_model_into_cache() {
        let root = temp_root("bootstrap-nemotron");
        let model = catalog_model("nemotron-speech-streaming-en-0.6b")
            .expect("catalog should include the default ASR model");
        let fetched_targets = Rc::new(RefCell::new(Vec::new()));
        let fetched_targets_clone = fetched_targets.clone();

        let readiness = bootstrap_model_with_fetch(model, &root, move |_url, target_path| {
            fetched_targets_clone
                .borrow_mut()
                .push(target_path.to_path_buf());
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(target_path, b"pepper-x-model").unwrap();
            Ok::<(), std::io::Error>(())
        })
        .expect("bootstrap should populate the model cache");

        assert!(readiness.is_ready);
        // MultiFile downloads fetch each file individually
        assert_eq!(fetched_targets.borrow().len(), 3);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_bootstraps_legacy_tdt_asr_model_into_cache() {
        let root = temp_root("bootstrap-tdt");
        let model = catalog_model("nemo-parakeet-tdt-0.6b-v3-int8")
            .expect("catalog should include the legacy ASR model");
        let fetched_targets = Rc::new(RefCell::new(Vec::new()));
        let fetched_targets_clone = fetched_targets.clone();

        let readiness = bootstrap_model_with_fetch(model, &root, move |_url, target_path| {
            fetched_targets_clone
                .borrow_mut()
                .push(target_path.to_path_buf());
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            write_tdt_asr_bundle(target_path);
            Ok::<(), std::io::Error>(())
        })
        .expect("bootstrap should populate the model cache");

        assert!(readiness.is_ready);
        assert_eq!(fetched_targets.borrow().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_reports_asr_and_cleanup_readiness_separately() {
        let root = temp_root("inventory");
        let cleanup = catalog_model("qwen3.5-2b-q4_k_m.gguf")
            .expect("catalog should include the default cleanup model");
        let cleanup_path = crate::model_install_dir(cleanup, &root);
        std::fs::create_dir_all(cleanup_path.parent().unwrap()).unwrap();
        write_cleanup_model_file(&cleanup_path);

        let inventory = model_inventory(&root);
        let asr = inventory
            .iter()
            .find(|entry| entry.id == "nemotron-speech-streaming-en-0.6b")
            .expect("inventory should include the ASR model");
        let cleanup = inventory
            .iter()
            .find(|entry| entry.id == "qwen3.5-2b-q4_k_m.gguf")
            .expect("inventory should include the cleanup model");

        assert!(!asr.readiness.is_ready);
        assert!(cleanup.readiness.is_ready);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_rejects_cleanup_models_without_gguf_header() {
        let root = temp_root("invalid-cleanup");
        let cleanup = catalog_model("qwen3.5-2b-q4_k_m.gguf")
            .expect("catalog should include the default cleanup model");
        let cleanup_path = crate::model_install_dir(cleanup, &root);
        std::fs::create_dir_all(cleanup_path.parent().unwrap()).unwrap();
        std::fs::write(&cleanup_path, b"not-a-gguf-model").unwrap();

        let readiness = model_readiness(cleanup, &root);

        assert!(!readiness.is_ready);
        assert_eq!(
            readiness.missing_files,
            vec!["qwen3.5-2b-q4_k_m.gguf".to_string()]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_rejects_archive_entries_with_unsafe_symlink_targets() {
        let root = temp_root("unsafe-symlink");
        let model = catalog_model("nemo-parakeet-tdt-0.6b-v3-int8")
            .expect("catalog should include the legacy ASR model");

        let error = bootstrap_model_with_fetch(model, &root, |_url, target_path| {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            write_tdt_asr_bundle_with_unsafe_symlink(target_path);
            Ok::<(), std::io::Error>(())
        })
        .unwrap_err();

        assert!(matches!(error, BootstrapError::InvalidArchiveEntry(_)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_offline_readiness_uses_cached_models_after_bootstrap() {
        let root = temp_root("offline");
        let model = catalog_model("nemotron-speech-streaming-en-0.6b")
            .expect("catalog should include the default ASR model");

        bootstrap_model_with_fetch(model, &root, |_url, target_path| {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(target_path, b"pepper-x-model").unwrap();
            Ok::<(), std::io::Error>(())
        })
        .expect("bootstrap should populate the model cache");

        let readiness = model_readiness(model, &root);

        assert!(readiness.is_ready);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_bootstrap_emits_progress_and_failure_states_for_default_models() {
        let root = temp_root("default-bootstrap-progress");
        let progress_snapshots = Rc::new(RefCell::new(Vec::new()));
        let progress_snapshots_clone = progress_snapshots.clone();

        let error = bootstrap_default_models_with_fetch(
            &root,
            move |progress| {
                progress_snapshots_clone.borrow_mut().push(progress.clone());
            },
            move |_url, target_path| {
                if target_path.to_string_lossy().contains(".gguf") {
                    if let Some(parent) = target_path.parent() {
                        std::fs::create_dir_all(parent).unwrap();
                    }
                    std::fs::write(target_path, b"GGUFcleanup").unwrap();
                    Ok::<(), std::io::Error>(())
                } else {
                    Err(std::io::Error::other("network down"))
                }
            },
        )
        .unwrap_err();

        let snapshots = progress_snapshots.borrow();
        assert!(matches!(error, BootstrapError::Fetch { .. }));
        assert!(snapshots
            .iter()
            .any(|snapshot| snapshot.model_states.iter().any(|state| {
                state.phase == BootstrapModelPhase::Downloading
                    && state.model_id == "nemotron-speech-streaming-en-0.6b"
            })));
        assert!(snapshots
            .last()
            .expect("failed bootstrap should emit a final snapshot")
            .model_states
            .iter()
            .any(|state| {
                state.phase == BootstrapModelPhase::Failed
                    && state.model_id == "nemotron-speech-streaming-en-0.6b"
            }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_bootstrap_retry_succeeds_without_restarting_the_plan() {
        let root = temp_root("default-bootstrap-retry");
        let attempts = Rc::new(RefCell::new(0usize));

        let first_attempt = bootstrap_default_models_with_fetch(&root, |_| {}, {
            let attempts = attempts.clone();
            move |_url, target_path| {
                *attempts.borrow_mut() += 1;
                if *attempts.borrow() == 1 {
                    return Err(std::io::Error::other("transient fetch failure"));
                }
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                if target_path.to_string_lossy().contains(".gguf") {
                    write_cleanup_model_file(target_path);
                } else {
                    // MultiFile: each target is a model file, not an archive
                    std::fs::write(target_path, b"pepper-x-model").unwrap();
                }
                Ok::<(), std::io::Error>(())
            }
        });
        assert!(first_attempt.is_err());

        let final_progress = bootstrap_default_models_with_fetch(
            &root,
            |_| {},
            move |_url, target_path| {
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                if target_path.to_string_lossy().contains(".gguf") {
                    write_cleanup_model_file(target_path);
                } else {
                    std::fs::write(target_path, b"pepper-x-model").unwrap();
                }
                Ok::<(), std::io::Error>(())
            },
        )
        .expect("retry should finish the default bootstrap plan");

        assert_eq!(final_progress.completed_models, final_progress.total_models);
        assert!(final_progress
            .model_states
            .iter()
            .all(|state| state.phase == BootstrapModelPhase::Ready));
        let _ = std::fs::remove_dir_all(root);
    }

    fn write_tdt_asr_bundle(target_path: &std::path::Path) {
        write_tdt_asr_bundle_with_prefix(
            target_path,
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        );
    }

    fn write_tdt_asr_bundle_with_prefix(target_path: &std::path::Path, prefix: &str) {
        let archive = std::fs::File::create(target_path).unwrap();
        let encoder = BzEncoder::new(archive, bzip2::Compression::best());
        let mut tar = Builder::new(encoder);

        for (file_name, contents) in [
            ("encoder.int8.onnx", b"encoder".as_slice()),
            ("decoder.int8.onnx", b"decoder".as_slice()),
            ("joiner.int8.onnx", b"joiner".as_slice()),
            ("tokens.txt", b"tokens".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, format!("{prefix}/{file_name}"), contents)
                .unwrap();
        }

        tar.into_inner().unwrap().flush().unwrap();
    }

    fn write_tdt_asr_bundle_with_unsafe_symlink(target_path: &std::path::Path) {
        let archive = std::fs::File::create(target_path).unwrap();
        let encoder = BzEncoder::new(archive, bzip2::Compression::best());
        let mut tar = Builder::new(encoder);

        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name("../../pepper-x-escape").unwrap();
        header.set_cksum();
        tar.append_data(
            &mut header,
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/unsafe-link",
            std::io::empty(),
        )
        .unwrap();

        tar.into_inner().unwrap().flush().unwrap();
    }

    fn write_cleanup_model_file(target_path: &std::path::Path) {
        std::fs::write(target_path, b"GGUFpepper-x-cleanup-model").unwrap();
    }
}
