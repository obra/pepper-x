use std::path::{Path, PathBuf};

use crate::catalog::{CatalogModel, InstallLayout};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReadiness {
    pub install_path: PathBuf,
    pub is_ready: bool,
    pub missing_files: Vec<String>,
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
        InstallLayout::File if install_path.is_file() => Vec::new(),
        InstallLayout::File => vec![model.id.to_string()],
    };

    ModelReadiness {
        install_path,
        is_ready: missing_files.is_empty(),
        missing_files,
    }
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
