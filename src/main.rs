use mev_template::run;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    // Initialize logger
    env_logger::init();

    println!("Starting the MEV bot...");

    run().await;

    println!("Bot stopped.");
}