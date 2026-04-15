fn main() {
    prost_build::Config::new()
        .compile_protos(&["proto/audit.proto"], &["proto/"])
        .unwrap();
}
