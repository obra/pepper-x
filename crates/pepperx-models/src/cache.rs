use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::catalog::{supported_models, CatalogModel, InstallLayout, ModelKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReadiness {
    pub install_path: PathBuf,
    pub is_ready: bool,
    pub missing_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInventoryEntry {
    pub id: String,
    pub kind: ModelKind,
    pub readiness: ModelReadiness,
}

pub fn default_cache_root() -> PathBuf {
    if let Some(xdg_cache_home) = nonempty_env_path("XDG_CACHE_HOME") {
        return xdg_cache_home.join("pepper-x").join("models");
    }

    if let Some(home) = nonempty_env_path("HOME") {
        return home.join(".cache").join("pepper-x").join("models");
    }

    PathBuf::from("pepper-x-models")
}

pub fn model_install_dir(model: &CatalogModel, cache_root: &Path) -> PathBuf {
    cache_root.join(model.install_path)
}

pub fn model_readiness(model: &CatalogModel, cache_root: &Path) -> ModelReadiness {
    let install_path = model_install_dir(model, cache_root);
    let missing_files = match model.install_layout {
        InstallLayout::Directory => model
            .required_files
            .iter()
            .filter(|file_name| !install_path.join(file_name).is_file())
            .map(|file_name| (*file_name).to_string())
            .collect(),
        InstallLayout::File if file_install_is_ready(model, &install_path) => Vec::new(),
        InstallLayout::File => vec![model.id.to_string()],
    };

    ModelReadiness {
        install_path,
        is_ready: missing_files.is_empty(),
        missing_files,
    }
}

fn file_install_is_ready(model: &CatalogModel, install_path: &Path) -> bool {
    if !install_path.is_file() {
        return false;
    }

    match model.kind {
        ModelKind::Cleanup => file_has_magic_prefix(install_path, b"GGUF"),
        _ => true,
    }
}

fn file_has_magic_prefix(path: &Path, expected: &[u8]) -> bool {
    let mut buffer = vec![0; expected.len()];
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    if file.read_exact(&mut buffer).is_err() {
        return false;
    }

    buffer == expected
}

pub fn model_inventory(cache_root: &Path) -> Vec<ModelInventoryEntry> {
    supported_models()
        .iter()
        .map(|model| ModelInventoryEntry {
            id: model.id.into(),
            kind: model.kind,
            readiness: model_readiness(model, cache_root),
        })
        .collect()
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
