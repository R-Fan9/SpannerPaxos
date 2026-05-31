fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .compile_protos(
            &[
                "proto/messages/save_write.proto",
                "proto/messages/commit_write.proto",
                "proto/messages/pre_vote.proto",
                "proto/messages/vote.proto",
                "proto/services/paxos_service.proto",
            ],
            &["proto/"],
        )?;
    Ok(())
}
