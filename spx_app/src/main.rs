use std::error::Error;

mod commands;
mod handles;
mod managers;
mod payloads;
mod services;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    Ok(())
}
