use std::error::Error;
use log::{info, error};

mod logger;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    // Initialize logger
    logger::init()?;

    info!("Starting the bot...");

    if let Err(e) = run().await {
        error!("Bot encountered an error: {}", e);
        return Err(Box::new(e));
    }

    info!("Bot stopped.");
    Ok(())
}