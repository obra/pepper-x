use llama_cpp_4::model::params::LlamaModelParams;
use pepperx_platform::gpu::get_gpu_info;

#[test]
fn test_gpu_detection() {
    let info = get_gpu_info();
    println!(
        "GPU detected: {:?} -> {} layers",
        info.vendor, info.recommended_layers
    );
    let _ = info.recommended_layers;
}

#[test]
fn test_model_params_cpu_only() {
    let params = LlamaModelParams::default().with_n_gpu_layers(0);
    assert_eq!(params.n_gpu_layers(), 0);
}

#[test]
fn test_model_params_gpu() {
    let info = get_gpu_info();
    if info.available {
        let params = LlamaModelParams::default().with_n_gpu_layers(info.recommended_layers);
        assert!(params.n_gpu_layers() > 0);
    }
}