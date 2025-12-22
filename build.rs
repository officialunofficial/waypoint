// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tell Cargo to rerun this build script if migrations change
    println!("cargo:rerun-if-changed=migrations");
    // Rerun if proto definitions change
    println!("cargo:rerun-if-changed=vendor/snapchain/proto/definitions");

    let proto_dir = "vendor/snapchain/proto/definitions";

    tonic_prost_build::configure()
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        // Add an attribute to silence large_enum_variant warnings in generated code
        .type_attribute(".", "#[allow(clippy::large_enum_variant)]")
        .build_server(false)
        .compile_protos(
            &[
                "vendor/snapchain/proto/definitions/admin_rpc.proto",
                "vendor/snapchain/proto/definitions/blocks.proto",
                "vendor/snapchain/proto/definitions/gossip.proto",
                "vendor/snapchain/proto/definitions/hub_event.proto",
                "vendor/snapchain/proto/definitions/message.proto",
                "vendor/snapchain/proto/definitions/node_state.proto",
                "vendor/snapchain/proto/definitions/onchain_event.proto",
                "vendor/snapchain/proto/definitions/request_response.proto",
                "vendor/snapchain/proto/definitions/rpc.proto",
                "vendor/snapchain/proto/definitions/sync_trie.proto",
                "vendor/snapchain/proto/definitions/username_proof.proto",
            ],
            &[proto_dir],
        )?;
    Ok(())
}
