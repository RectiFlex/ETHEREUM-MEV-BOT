use ethers::prelude::*;
use std::sync::Arc;
use crate::Config;

#[derive(Debug)]
pub struct EnhancedSandwichStrategy {
    config: Arc<Config>,
    min_profit_wei: U256,
    max_position_size: U256,
    slippage_tolerance: u64,
    gas_price_premium: U256,
}

impl EnhancedSandwichStrategy {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            min_profit_wei: U256::from(5) * U256::from(10).pow(U256::from(16)), // 0.05 ETH minimum
            max_position_size: U256::from(50) * U256::from(10).pow(U256::from(18)), // 50 ETH max
            slippage_tolerance: 300, // 3% slippage tolerance
            gas_price_premium: U256::from(2_000_000_000u64), // 2 gwei premium
        }
    }

    pub fn calculate_safe_gas_prices(&self, victim_gas_price: Option<U256>) -> (U256, U256) {
        let base_price = victim_gas_price.unwrap_or(U256::from(20_000_000_000u64)); // 20 gwei default
        
        // Frontrun: Add premium, but check for overflow
        let frontrun_price = base_price.saturating_add(self.gas_price_premium);
        
        // Backrun: Subtract premium, but ensure we don't underflow
        let backrun_price = if base_price > self.gas_price_premium {
            base_price - self.gas_price_premium
        } else {
            base_price / 2 // If too low, use half the price
        };
        
        (frontrun_price, backrun_price)
    }

    pub fn validate_victim_transaction(&self, tx: &Transaction) -> bool {
        // Check if transaction has sufficient value
        if tx.value < U256::from(10).pow(U256::from(16)) { // Less than 0.01 ETH
            return false;
        }
        
        // Check gas price is reasonable
        if let Some(gas_price) = tx.gas_price {
            if gas_price > U256::from(500_000_000_000u64) { // Over 500 gwei
                return false;
            }
        }
        
        // Check if to address exists (not contract creation)
        tx.to.is_some()
    }

    pub async fn calculate_advanced_sandwich(
        &self,
        victim_amount: U256,
        reserve_in: U256,
        reserve_out: U256,
        _token_decimals: u8,
    ) -> Option<OptimalSandwich> {
        // Use Newton's method for more accurate optimization
        let mut x = reserve_in / 20; // Start with 5% of reserves
        let mut best_profit = U256::zero();
        let mut best_x = U256::zero();
        
        for _ in 0..10 { // 10 iterations of Newton's method
            let (profit, derivative) = self.calculate_profit_and_derivative(
                x, victim_amount, reserve_in, reserve_out
            );
            
            if profit > best_profit {
                best_profit = profit;
                best_x = x;
            }
            
            // Newton's method update
            if derivative > U256::from(1000) {
                let adjustment = profit * U256::from(10).pow(U256::from(18)) / derivative;
                x = x.saturating_add(adjustment / U256::from(10).pow(U256::from(18)));
            } else {
                break;
            }
            
            // Ensure x doesn't exceed max position or reserves
            x = x.min(self.max_position_size).min(reserve_in / 5);
        }
        
        if best_profit < self.min_profit_wei {
            return None;
        }
        
        Some(OptimalSandwich {
            frontrun_amount: best_x,
            backrun_amount: best_x * 98 / 100, // 2% slippage buffer
            profit: best_profit,
            gas_cost: self.estimate_gas_cost().await,
            price_impact: (best_x.as_u128() as f64) / (reserve_in.as_u128() as f64),
        })
    }

    fn calculate_profit_and_derivative(
        &self,
        x: U256,
        victim_amount: U256,
        r_in: U256,
        r_out: U256,
    ) -> (U256, U256) {
        // Calculate profit using exact AMM formula
        let profit = self.calculate_simple_profit(x, victim_amount, r_in, r_out);
        
        // Approximate derivative using finite differences
        let h = x / 1000 + 1;
        let profit_plus = self.calculate_simple_profit(x + h, victim_amount, r_in, r_out);
        let derivative = profit_plus.saturating_sub(profit) / h;
        
        (profit, derivative)
    }

    fn calculate_simple_profit(
        &self,
        x: U256,
        victim_amount: U256,
        r_in: U256,
        r_out: U256,
    ) -> U256 {
        let k = r_in * r_out;
        let new_r_in = r_in + x;
        if new_r_in == U256::zero() {
            return U256::zero();
        }
        
        let new_r_out = k / new_r_in;
        let amount_out = r_out.saturating_sub(new_r_out);
        
        let new_r_in_2 = new_r_in + victim_amount;
        if new_r_in_2 == U256::zero() {
            return U256::zero();
        }
        
        let new_r_out_2 = k / new_r_in_2;
        
        let final_r_out = new_r_out_2 + amount_out * 997 / 1000;
        if final_r_out == U256::zero() {
            return U256::zero();
        }
        
        let final_r_in = k / final_r_out;
        let amount_back = new_r_in_2.saturating_sub(final_r_in);
        
        amount_back.saturating_sub(x)
    }

    async fn estimate_gas_cost(&self) -> U256 {
        let base_fee = self.config.http
            .get_block(BlockNumber::Latest)
            .await
            .ok()
            .and_then(|b| b)
            .and_then(|b| b.base_fee_per_gas)
            .unwrap_or(U256::from(30_000_000_000u64)); // 30 gwei default
        
        // Sandwich typically uses ~400k gas total
        U256::from(400_000) * base_fee
    }
}

#[derive(Debug)]
pub struct OptimalSandwich {
    pub frontrun_amount: U256,
    pub backrun_amount: U256,
    pub profit: U256,
    pub gas_cost: U256,
    pub price_impact: f64,
}
