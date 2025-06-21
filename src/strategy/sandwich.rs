use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::abi::AbiDecode;
use std::sync::Arc;
use crate::{Config, address_book::UniV2RouterCalls, uni};
use super::types::*;

#[derive(Debug)]
pub struct SandwichStrategy {
    config: Arc<Config>,
    min_profit_wei: U256,
}

impl SandwichStrategy {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            min_profit_wei: U256::from(10).pow(U256::from(17)), // 0.1 ETH minimum profit
        }
    }


    fn calculate_frontrun_gas_price(&self, victim_tx: &Transaction) -> U256 {
        let base_price = victim_tx.gas_price.unwrap_or(U256::from(20_000_000_000u64)); // 20 gwei default
        // Safely add premium without overflow
        base_price.saturating_add(U256::from(2_000_000_000u64)) // 2 gwei premium
    }

    fn calculate_backrun_gas_price(&self, victim_tx: &Transaction) -> U256 {
        let base_price = victim_tx.gas_price.unwrap_or(U256::from(20_000_000_000u64)); // 20 gwei default
        // Safely subtract premium without underflow
        if base_price > U256::from(2_000_000_000u64) {
            base_price - U256::from(2_000_000_000u64)
        } else {
            base_price / 2 // If too low, use half the price
        }
    }

    fn validate_profitable_victim(&self, tx: &Transaction, min_value: U256) -> bool {
        // Skip transactions with very low value
        if tx.value < min_value {
            return false;
        }
        
        // Skip transactions with unreasonable gas prices
        if let Some(gas_price) = tx.gas_price {
            if gas_price > U256::from(500_000_000_000u64) || gas_price == U256::zero() {
                return false;
            }
        }
        
        true
    }
    pub async fn analyze(&self, tx: &Transaction) -> Vec<MEVOpportunity> {
        let mut opportunities = Vec::new();

        // Decode router calls
        if let Ok(decoded) = UniV2RouterCalls::decode(&tx.input) {
            match decoded {
                UniV2RouterCalls::SwapExactETHForTokens(call) => {
                    if let Some(opp) = self.analyze_eth_to_token_swap(tx, call.path, call.amount_out_min).await {
                        opportunities.push(opp);
                    }
                },
                UniV2RouterCalls::SwapExactETHForTokensSupportingFeeOnTransferTokens(call) => {
                    if let Some(opp) = self.analyze_eth_to_token_swap(tx, call.path, call.amount_out_min).await {
                        opportunities.push(opp);
                    }
                },
                UniV2RouterCalls::SwapExactTokensForETH(call) => {
                    if let Some(opp) = self.analyze_token_to_eth_swap(tx, call.path, call.amount_in, call.amount_out_min).await {
                        opportunities.push(opp);
                    }
                },
                UniV2RouterCalls::SwapExactTokensForETHSupportingFeeOnTransferTokens(call) => {
                    if let Some(opp) = self.analyze_token_to_eth_swap(tx, call.path, call.amount_in, call.amount_out_min).await {
                        opportunities.push(opp);
                    }
                },
                UniV2RouterCalls::SwapExactTokensForTokens(call) => {
                    if let Some(opp) = self.analyze_token_to_token_swap(tx, call.path, call.amount_in, call.amount_out_min).await {
                        opportunities.push(opp);
                    }
                },
                UniV2RouterCalls::SwapExactTokensForTokensSupportingFeeOnTransferTokens(call) => {
                    if let Some(opp) = self.analyze_token_to_token_swap(tx, call.path, call.amount_in, call.amount_out_min).await {
                        opportunities.push(opp);
                    }
                },
                _ => {}
            }
        }

        opportunities
    }

    async fn analyze_token_to_eth_swap(
        &self,
        _victim_tx: &Transaction,
        _path: Vec<Address>,
        _amount_in: U256,
        _amount_out_min: U256,
    ) -> Option<MEVOpportunity> {
        if _path.len() < 2 {
            return None;
        }

        let token_in = _path[0];
        let weth = _path[_path.len() - 1];
        
        // Get pool info
        let pool_address = self.get_pair_address(token_in, weth);
        let (reserve0, reserve1) = self.get_reserves(pool_address).await?;
        
        // Calculate optimal sandwich amounts
        let optimal_sandwich = self.calculate_optimal_sandwich(
            _amount_in,
            reserve0,
            reserve1,
            true, // token to ETH
        );

        if optimal_sandwich.profit < self.min_profit_wei {
            return None;
        }

        // Build frontrun and backrun transactions
        let frontrun_tx = self.build_frontrun_tx(
            token_in,
            weth,
            optimal_sandwich.frontrun_amount,
            _victim_tx,
        );
        
        let backrun_tx = self.build_backrun_tx(
            token_in,
            weth,
            optimal_sandwich.backrun_amount,
            _victim_tx,
        );

        Some(MEVOpportunity {
            id: format!("sandwich_{}", _victim_tx.hash),
            target_tx: _victim_tx.clone(),
            strategy_type: StrategyType::Sandwich(SandwichDetails {
                victim_tx: _victim_tx.clone(),
                frontrun_tx,
                backrun_tx,
                target_pool: pool_address,
                token_in,
                token_out: weth,
                optimal_amount: optimal_sandwich.frontrun_amount,
                victim_amount_in: _amount_in,
                victim_amount_out_min: _amount_out_min,
                price_impact: optimal_sandwich.price_impact,
            }),
            estimated_profit: optimal_sandwich.profit,
            gas_cost: optimal_sandwich.gas_cost,
            priority: self.calculate_priority(&optimal_sandwich),
            expiry_block: self.get_current_block().await + 1,
        })
    }

    async fn analyze_eth_to_token_swap(
        &self,
        _victim_tx: &Transaction,
        _path: Vec<Address>,
        _amount_out_min: U256,
    ) -> Option<MEVOpportunity> {
        // Similar implementation for ETH to token swaps
        // Frontrun by buying tokens with ETH, backrun by selling tokens for ETH
        None // Simplified for brevity
    }

    async fn analyze_token_to_token_swap(
        &self,
        _victim_tx: &Transaction,
        _path: Vec<Address>,
        _amount_in: U256,
        _amount_out_min: U256,
    ) -> Option<MEVOpportunity> {
        // Multi-hop sandwich attacks
        None // Simplified for brevity
    }

    fn calculate_optimal_sandwich(
        &self,
        victim_amount: U256,
        reserve_in: U256,
        reserve_out: U256,
        _is_token_to_eth: bool,
    ) -> OptimalSandwich {
        // Advanced sandwich calculation using binary search
        let mut low = U256::from(0);
        let mut high = reserve_in / 10; // Max 10% of pool
        let mut best_profit = U256::from(0);
        let mut best_amount = U256::from(0);
        
        while low <= high {
            let mid = (low + high) / 2;
            
            // Simulate sandwich attack
            let (profit, gas_cost) = self.simulate_sandwich_profit(
                mid,
                victim_amount,
                reserve_in,
                reserve_out,
            );
            
            if profit > best_profit {
                best_profit = profit;
                best_amount = mid;
            }
            
            // Binary search logic
            if profit > gas_cost {
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }
        
        OptimalSandwich {
            frontrun_amount: best_amount,
            backrun_amount: best_amount * 95 / 100, // Account for slippage
            profit: best_profit,
            gas_cost: U256::from(500000) * U256::from(50) * U256::from(10).pow(U256::from(9)), // Estimate
            price_impact: (best_amount.as_u64() as f64) / (reserve_in.as_u64() as f64),
        }
    }

    fn simulate_sandwich_profit(
        &self,
        frontrun_amount: U256,
        victim_amount: U256,
        reserve_in: U256,
        reserve_out: U256,
    ) -> (U256, U256) {
        // Step 1: Frontrun transaction
        let (frontrun_out, new_reserve_in, new_reserve_out) = uni::get_amount_out(
            frontrun_amount,
            reserve_in,
            reserve_out,
        );
        
        // Step 2: Victim transaction
        let (_, new_reserve_in_2, new_reserve_out_2) = uni::get_amount_out(
            victim_amount,
            new_reserve_in,
            new_reserve_out,
        );
        
        // Step 3: Backrun transaction (sell back)
        let (backrun_out, _, _) = uni::get_amount_out(
            frontrun_out,
            new_reserve_out_2,
            new_reserve_in_2,
        );
        
        // Calculate profit
        let profit = if backrun_out > frontrun_amount {
            backrun_out - frontrun_amount
        } else {
            U256::from(0)
        };
        
        let gas_cost = U256::from(300000) * U256::from(50) * U256::from(10).pow(U256::from(9));
        
        (profit, gas_cost)
    }

    fn build_frontrun_tx(
        &self,
        _token_in: Address,
        _token_out: Address,
        _amount: U256,
        _victim_tx: &Transaction,
    ) -> TypedTransaction {
        // Build the frontrun transaction
        let mut tx = TypedTransaction::default();
        tx.set_to(_victim_tx.to.unwrap())
            .set_value(_amount)
            .set_gas(U256::from(300000))
            .set_gas_price(self.calculate_frontrun_gas_price(_victim_tx));
        
        tx
    }

    fn build_backrun_tx(
        &self,
        _token_in: Address,
        _token_out: Address,
        _amount: U256,
        _victim_tx: &Transaction,
    ) -> TypedTransaction {
        // Build the backrun transaction
        let mut tx = TypedTransaction::default();
        tx.set_to(_victim_tx.to.unwrap())
            .set_gas(U256::from(300000))
            .set_gas_price(self.calculate_backrun_gas_price(_victim_tx));
        
        tx
    }

    fn get_pair_address(&self, _token0: Address, _token1: Address) -> Address {
        // Calculate Uniswap V2 pair address
        // In production, this should use CREATE2 calculation
        Address::zero() // Placeholder
    }

    async fn get_reserves(&self, _pool: Address) -> Option<(U256, U256)> {
        // Get pool reserves from chain
        // In production, this should call the pool contract
        Some((U256::from(1000000), U256::from(2000000))) // Placeholder
    }

    async fn get_current_block(&self) -> U64 {
        self.config.http.get_block_number().await.unwrap_or_default()
    }

    fn calculate_priority(&self, sandwich: &OptimalSandwich) -> u8 {
        // Higher profit = higher priority
        if sandwich.profit > U256::from(10).pow(U256::from(18)) {
            10
        } else if sandwich.profit > U256::from(5) * U256::from(10).pow(U256::from(17)) {
            8
        } else {
            5
        }
    }
}

#[derive(Debug)]
struct OptimalSandwich {
    frontrun_amount: U256,
    backrun_amount: U256,
    profit: U256,
    gas_cost: U256,
    price_impact: f64,
} 