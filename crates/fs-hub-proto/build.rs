//! Schema compilation (ADR 0040): `capnpc-embedded` (the official compiler as WASM, no C++ toolchain)
//! produces the `CodeGeneratorRequest`, `capnpc` turns it into Rust.
//!
//! The generated files are **committed** (`src/*_capnp.rs`), so a normal build compiles nothing here.
//! After a schema change run: `cargo build -p fs-hub-proto --features regenerate`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../schema");
    #[cfg(feature = "regenerate")]
    regenerate();
}

#[cfg(feature = "regenerate")]
fn regenerate() {
    let request = capnpc_embedded::CompileCommand::new()
        .file("../../schema/hub.capnp")
        .file("../../schema/common.capnp")
        .src_prefix("../../schema")
        .compile()
        .expect("schema compilation failed");
    let out = std::env::var("OUT_DIR").unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(&out)
        .run(&request[..])
        .expect("code generation failed");
    for name in ["hub_capnp.rs", "common_capnp.rs"] {
        std::fs::copy(format!("{out}/{name}"), format!("src/{name}")).expect("copy generated file");
    }
}
