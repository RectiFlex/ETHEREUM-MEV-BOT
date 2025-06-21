use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct MEVOpportunity {
    pub id: String,
    pub target_tx: Transaction,
    pub strategy_type: StrategyType,
    pub estimated_profit: U256,
    pub gas_cost: U256,
    pub priority: u8,
    pub expiry_block: U64,
}

#[derive(Debug, Clone)]
pub enum StrategyType {
    Sandwich(SandwichDetails),
    Arbitrage(ArbitrageDetails),
}

#[derive(Debug, Clone)]
pub struct SandwichDetails {
    pub victim_tx: Transaction,
    pub frontrun_tx: TypedTransaction,
    pub backrun_tx: TypedTransaction,
    pub target_pool: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub optimal_amount: U256,
    pub victim_amount_in: U256,
    pub victim_amount_out_min: U256,
    pub price_impact: f64,
}

#[derive(Debug, Clone)]
pub struct ArbitrageDetails {
    pub path: Vec<Address>,
    pub pools: Vec<PoolInfo>,
    pub amount_in: U256,
    pub expected_profit: U256,
    pub gas_estimate: U256,
}

#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
    pub reserve0: U256,
    pub reserve1: U256,
    pub fee: u16,
    pub dex_type: DexType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DexType {
    UniswapV2,
    UniswapV3,
    SushiSwap,
    PancakeSwap,
    Custom(u8),
}

#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub success: bool,
    pub profit: U256,
    pub gas_used: U256,
    pub revert_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleTransaction {
    pub signer: Address,
    pub tx: TypedTransaction,
    pub can_revert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub txs: Vec<BundleTransaction>,
    pub block_number: U64,
} 