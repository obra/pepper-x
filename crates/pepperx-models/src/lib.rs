mod cache;
mod catalog;
mod download;

pub use cache::{default_cache_root, model_install_dir, model_readiness, ModelReadiness};
pub use catalog::{supported_models, CatalogModel, InstallLayout, ModelKind};
pub use download::{download_support, DownloadSupport};

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static std::sync::Mutex<()> {
        static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        ENV_LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    fn set_or_remove_env_var(name: &str, value: Option<OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pepper-x-models-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn catalog_lists_supported_asr_and_cleanup_models() {
        let models = supported_models();

        assert!(models.iter().any(|model| {
            model.id == "nemo-parakeet-tdt-0.6b-v2-int8" && model.kind == ModelKind::Asr
        }));
        assert!(models.iter().any(|model| {
            model.id == "qwen2.5-3b-instruct-q4_k_m.gguf" && model.kind == ModelKind::Cleanup
        }));
    }

    #[test]
    fn cache_root_prefers_xdg_cache_home() {
        let _guard = env_lock().lock().unwrap();
        let xdg_cache_home = temp_root("xdg-cache-home");
        let previous_xdg_cache_home = std::env::var_os("XDG_CACHE_HOME");
        let previous_home = std::env::var_os("HOME");
        std::fs::create_dir_all(&xdg_cache_home).unwrap();
        std::env::set_var("XDG_CACHE_HOME", &xdg_cache_home);
        std::env::remove_var("HOME");

        let cache_root = default_cache_root();

        assert_eq!(cache_root, xdg_cache_home.join("pepper-x").join("models"));
        set_or_remove_env_var("XDG_CACHE_HOME", previous_xdg_cache_home);
        set_or_remove_env_var("HOME", previous_home);
        let _ = std::fs::remove_dir_all(xdg_cache_home);
    }

    #[test]
    fn cache_reports_missing_vs_installed_model_readiness() {
        let cache_root = temp_root("model-readiness");
        std::fs::create_dir_all(&cache_root).unwrap();
        let model = supported_models()
            .iter()
            .find(|model| model.id == "nemo-parakeet-tdt-0.6b-v2-int8")
            .expect("catalog should include the default ASR model");

        let missing = model_readiness(model, &cache_root);
        assert!(!missing.is_ready);
        assert!(missing
            .missing_files
            .contains(&String::from("encoder.int8.onnx")));

        let install_dir = model_install_dir(model, &cache_root);
        std::fs::create_dir_all(&install_dir).unwrap();
        for file_name in [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ] {
            std::fs::write(install_dir.join(file_name), b"pepper-x").unwrap();
        }

        let ready = model_readiness(model, &cache_root);

        assert!(ready.is_ready);
        assert!(ready.missing_files.is_empty());
        let _ = std::fs::remove_dir_all(cache_root);
    }
}
