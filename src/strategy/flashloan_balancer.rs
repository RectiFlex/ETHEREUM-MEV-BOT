use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use std::sync::Arc;
use crate::Config;
use super::types::*;

#[derive(Debug)]
pub struct FlashloanBalancerStrategy {
    config: Arc<Config>,
    flashloan_provider: Address,
    balancer_vault: Address,
    min_profit: U256,
}

impl FlashloanBalancerStrategy {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            flashloan_provider: "0x7d2768dE32b0b80b7a3454c06Bdac2DCf34d8a51".parse().unwrap(), // Aave V2 pool
            balancer_vault: "0xBA12222222228d8Ba445958a75a0704d566BF2C8".parse().unwrap(), // Balancer vault
            config,
            min_profit: U256::from(10).pow(U256::from(17)), // 0.1 ETH
        }
    }

    pub async fn analyze(&self, tx: &Transaction) -> Vec<MEVOpportunity> {
        let mut ops = Vec::new();
        if tx.value < self.min_profit { return ops; }
        if let Some(opp) = self.build_flashloan_sandwich(tx).await { ops.push(opp); }
        ops
    }

    async fn build_flashloan_sandwich(&self, victim_tx: &Transaction) -> Option<MEVOpportunity> {
        let flashloan_tx = self.build_flashloan_tx(victim_tx);
        let repay_tx = self.build_repay_tx();
        Some(MEVOpportunity {
            id: format!("flashloan_balancer_{:?}", victim_tx.hash),
            target_tx: victim_tx.clone(),
            strategy_type: StrategyType::Sandwich(SandwichDetails {
                victim_tx: victim_tx.clone(),
                frontrun_tx: flashloan_tx,
                backrun_tx: repay_tx,
                target_pool: self.balancer_vault,
                token_in: Address::zero(),
                token_out: Address::zero(),
                optimal_amount: U256::zero(),
                victim_amount_in: victim_tx.value,
                victim_amount_out_min: U256::zero(),
                price_impact: 0.0,
            }),
            estimated_profit: self.min_profit,
            gas_cost: U256::from(750_000),
            priority: 7,
            expiry_block: self.get_current_block().await + 1,
        })
    }

    fn build_flashloan_tx(&self, victim_tx: &Transaction) -> TypedTransaction {
        let mut tx = TypedTransaction::default();
        tx.set_to(self.flashloan_provider)
            .set_data(Bytes::from_static(b"flashLoan"))
            .set_gas(U256::from(500_000))
            .set_gas_price(victim_tx.gas_price.unwrap_or_default());
        tx
    }

    fn build_repay_tx(&self) -> TypedTransaction {
        let mut tx = TypedTransaction::default();
        tx.set_to(self.flashloan_provider)
            .set_data(Bytes::from_static(b"repay"))
            .set_gas(U256::from(300_000));
        tx
    }

    async fn get_current_block(&self) -> U64 {
        self.config.http.get_block_number().await.unwrap_or_default()
    }
}
