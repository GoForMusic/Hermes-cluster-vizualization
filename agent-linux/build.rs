fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../proto/cri");
    // client and server: the server stands in for containerd in the tests
    tonic_prost_build::configure()
        .compile_protos(&["../proto/cri/v1/runtime.proto"], &["../proto/cri"])?;
    Ok(())
}
