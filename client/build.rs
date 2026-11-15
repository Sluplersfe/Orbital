fn main() {
    println!("cargo:rerun-if-changed=../proto/orbital.proto");
    prost_build::compile_protos(&["../proto/orbital.proto"], &["../proto"]).expect("protoc");
}
