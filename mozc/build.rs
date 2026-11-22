// Mozc의 protobuf 정의(proto/, BSD-3, LICENSE-mozc)에서 Rust 코드를 만든다.
// Mozc는 proto2 group(repeated group Segment 등)을 쓰므로 group을 지원하는 prost를 쓰고,
// protoc는 protoc-bin-vendored가 넣어 준 실행 파일을 쓴다.
fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    // SAFETY: 빌드 스크립트는 단일 스레드에서 실행된다.
    unsafe { std::env::set_var("PROTOC", protoc) };
    prost_build::Config::new()
        .compile_protos(
            &[
                "proto/protocol/commands.proto",
                "proto/protocol/config.proto",
                "proto/protocol/candidate_window.proto",
                "proto/protocol/engine_builder.proto",
                "proto/ipc/ipc.proto",
            ],
            &["proto"],
        )
        .expect("compile Mozc protos");
    println!("cargo:rerun-if-changed=proto");
}
