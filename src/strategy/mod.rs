pub mod sandwich;
pub mod arbitrage;
pub mod types;
pub mod simulator;
pub mod bundle;

use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::Config;

pub use types::*;
pub use sandwich::SandwichStrategy;
pub use arbitrage::ArbitrageStrategy;
pub use simulator::TxSimulator;
pub use bundle::BundleBuilder;

#[derive(Debug, Clone)]
pub struct StrategyManager {
    sandwich: Arc<RwLock<SandwichStrategy>>,
    arbitrage: Arc<RwLock<ArbitrageStrategy>>,
    simulator: Arc<TxSimulator>,
    bundle_builder: Arc<BundleBuilder>,
    config: Arc<Config>,
}

impl StrategyManager {
    pub async fn new(config: Arc<Config>) -> Self {
        let simulator = Arc::new(TxSimulator::new(config.http.clone()));
        let bundle_builder = Arc::new(BundleBuilder::new(config.http.clone()));
        
        Self {
            sandwich: Arc::new(RwLock::new(SandwichStrategy::new(config.clone()))),
            arbitrage: Arc::new(RwLock::new(ArbitrageStrategy::new(config.clone()))),
            simulator,
            bundle_builder,
            config,
        }
    }

    pub async fn analyze_transaction(&self, tx: &Transaction) -> Vec<MEVOpportunity> {
        let mut opportunities = Vec::new();

        // Run strategies in parallel
        let sandwich_lock = self.sandwich.read().await;
        let arb_lock = self.arbitrage.read().await;
        
        let (sandwich_ops, arb_ops) = tokio::join!(
            sandwich_lock.analyze(tx),
            arb_lock.analyze(tx)
        );

        opportunities.extend(sandwich_ops);
        opportunities.extend(arb_ops);

        // Simulate and filter profitable opportunities
        let mut profitable_ops = Vec::new();
        for op in opportunities {
            if let Ok(sim_result) = self.simulator.simulate(&op).await {
                if sim_result.profit > U256::from(0) {
                    profitable_ops.push(op);
                }
            }
        }

        profitable_ops
    }

    pub async fn execute_opportunity(&self, opportunity: &MEVOpportunity) -> Result<TxHash, Box<dyn std::error::Error + Send + Sync>> {
        match &opportunity.strategy_type {
            StrategyType::Sandwich(details) => {
                let bundle = self.bundle_builder.build_sandwich_bundle(
                    &opportunity.target_tx,
                    details,
                    opportunity.estimated_profit
                ).await?;
                
                self.bundle_builder.send_bundle(bundle).await
            },
            StrategyType::Arbitrage(details) => {
                let tx = self.bundle_builder.build_arbitrage_tx(
                    details,
                    opportunity.estimated_profit
                ).await?;
                
                let pending = self.config.http.send_transaction(tx, None).await?;
                Ok(pending.tx_hash())
            }
        }
    }
} 
pub mod enhanced_sandwich;
pub mod advanced_features;

pub use enhanced_sandwich::EnhancedSandwichStrategy;
pub use advanced_features::AdvancedMEVFeatures;

impl StrategyManager {
    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }
}
