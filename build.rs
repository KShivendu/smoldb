use std::{env, path::PathBuf};

pub fn build_proto_files() {
    let build_out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    tonic_build::configure()
        .file_descriptor_set_path(build_out_dir.join("smoldb_descriptor.bin")) // pushed to target/debug/build/smoldb-<hash>/out/smoldb_descriptor.bin
        .out_dir("src/api/grpc")
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["src/api/grpc/proto/smoldb.proto"],
            &["src/api/grpc/proto"],
        )
        .expect("Failed to compile proto files");

    let generated_file = PathBuf::from("src/api/grpc/smoldb.rs");
    let target_file = PathBuf::from("src/api/grpc/schema.rs");

    if generated_file.exists() {
        std::fs::rename(generated_file, target_file)
            .expect("Failed to rename generated file to schema.rs");
    } else {
        panic!(
            "Generated file does not exist: {}",
            generated_file.display()
        );
    }
}

fn main() {
    build_proto_files();

    println!("Proto files compiled successfully.");
}
