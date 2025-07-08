use std::{env, path::PathBuf};

pub fn build_proto_files() {
    let build_out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    tonic_build::configure()
        .file_descriptor_set_path(build_out_dir.join("smoldb_descriptor.bin")) // pushed to target/debug/build/smoldb-<hash>/out/smoldb_descriptor.bin
        .out_dir("src/api/grpc")
        .build_server(true)
        .build_client(true)
        .compile_protos(&["src/proto/p2p_grpc.proto"], &["src/proto"])
        .expect("Failed to compile proto files");
}

fn main() {
    build_proto_files();

    println!("Proto files compiled successfully.");
}
