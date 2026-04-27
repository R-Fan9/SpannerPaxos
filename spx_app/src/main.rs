use std::error::Error;

mod commands;
mod configs;
mod handles;
mod managers;
mod services;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    Ok(())
}
