// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tell Cargo to rerun this build script if migrations change
    println!("cargo:rerun-if-changed=migrations");

    // Use out_dir to set a different output directory for admin_rpc.proto
    tonic_prost_build::configure()
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        // Add an attribute to silence large_enum_variant warnings in generated code
        .type_attribute(".", "#[allow(clippy::large_enum_variant)]")
        .build_server(false)
        .compile_protos(
            &[
                "src/proto/admin_rpc.proto",
                "src/proto/blocks.proto",
                "src/proto/gossip.proto",
                "src/proto/hub_event.proto",
                "src/proto/message.proto",
                "src/proto/node_state.proto",
                "src/proto/onchain_event.proto",
                "src/proto/request_response.proto",
                "src/proto/rpc.proto",
                "src/proto/sync_trie.proto",
                "src/proto/username_proof.proto",
            ],
            &["src/proto"],
        )?;
    Ok(())
}
