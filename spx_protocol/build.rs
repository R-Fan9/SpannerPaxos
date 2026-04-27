fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .compile_protos(
            &[
                "proto/services/follower_service.proto",
                "proto/services/leader_service.proto",
            ],
            &["proto/services"],
        )?;
    Ok(())
}
