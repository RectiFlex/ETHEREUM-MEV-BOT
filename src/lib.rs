pub mod address_book;
pub mod alert;
pub mod block_scanner;
pub mod dex;
pub mod helpers;
pub mod mempool;
pub mod uni;
pub mod strategy;

use std::sync::Arc;

use address_book::*;
use ethers::prelude::k256::ecdsa::SigningKey;
use ethers::prelude::*;
use helpers::address;
use strategy::StrategyManager;

use crate::dex::Dex;
use crate::helpers::setup_signer;

#[derive(Debug)]
pub struct Config {
    pub http: Arc<SignerMiddleware<Provider<Http>, Wallet<SigningKey>>>,
    pub wss: Arc<Provider<Ws>>,
}

impl Config {
    pub async fn new() -> Self {
        let network = std::env::var("NETWORK_RPC").expect("missing NETWORK_RPC");
        let provider: Provider<Http> = Provider::<Http>::try_from(network).unwrap();
        let middleware = Arc::new(setup_signer(provider.clone()).await);

        let ws_network = std::env::var("NETWORK_WSS").expect("missing NETWORK_WSS");
        let ws_provider: Provider<Ws> = Provider::<Ws>::connect(ws_network).await.unwrap();
        Self {
            http: middleware,
            wss: Arc::new(ws_provider),
        }
    }

    pub async fn create_dex(&self, factory: Address, router: Address) -> Dex {
        Dex::new(self.http.clone(), factory, router)
    }
}

/// Run the MEV bot with advanced strategies
pub async fn run() {
    println!("🚀 Starting MEV Bot - Jaredfromsubway Style");
    
    let config = Arc::new(Config::new().await);
    
    // Initialize strategy manager
    let strategy_manager = Arc::new(StrategyManager::new(config.clone()).await);
    
    // Display configuration
    println!("📊 Configuration:");
    println!("   - Network RPC: {}", std::env::var("NETWORK_RPC").unwrap_or_default());
    println!("   - Min Profit: 0.1 ETH");
    println!("   - Strategies: Sandwich Attack, Cross-DEX Arbitrage");
    println!("   - Bundle Submission: Flashbots");
    
    // Example of how to interact with a DEX (optional)
    let spooky_factory = address(SPOOKY_SWAP_FACTORY);
    let spooky_router = address(SPOOKY_SWAP_ROUTER);
    let dex = config.create_dex(spooky_factory, spooky_router).await;
    dex.get_pairs().await;

    // Thread for checking what block we're on
    let config_clone = config.clone();
    tokio::spawn(async move {
        block_scanner::loop_blocks(Arc::clone(&config_clone.http)).await;
    });

    // Main MEV monitoring loop with strategy execution
    enhanced_mempool::enhanced_mempool_monitor(Arc::clone(&config.wss), strategy_manager).await;
}
pub mod enhanced_mempool;
