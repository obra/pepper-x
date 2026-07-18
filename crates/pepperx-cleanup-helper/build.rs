fn main() {
    if cfg!(target_os = "macos") {
        return;
    }

    // llama-cpp-sys-4 links ggml-cuda statically but does not propagate CUDA toolkit
    // libraries. Use link-args (not link-lib) so they appear after the .cu objects in
    // libllama_cpp_sys_4.rlib — required for relocatable device code.
    let cuda_root = std::env::var("CUDA_HOME")
        .or_else(|_| std::env::var("CUDA_PATH"))
        .unwrap_or_else(|_| "/usr/local/cuda".to_string());

    println!("cargo:rustc-link-search=native={cuda_root}/lib64");
    println!("cargo:rustc-link-search=native={cuda_root}/lib");

    println!("cargo:rustc-link-arg=-Wl,--start-group");
    println!("cargo:rustc-link-arg=-lcudadevrt");
    println!("cargo:rustc-link-arg=-lcudart_static");
    println!("cargo:rustc-link-arg=-lcublas_static");
    println!("cargo:rustc-link-arg=-lcublasLt_static");
    println!("cargo:rustc-link-arg=-lculibos");
    println!("cargo:rustc-link-arg=-Wl,--end-group");
    println!("cargo:rustc-link-arg=-lcuda");
}