use ethers::prelude::*;
use std::sync::Arc;
use std::collections::HashMap;
use crate::{Config, uni};
use super::types::*;

#[derive(Debug)]
pub struct ArbitrageStrategy {
    config: Arc<Config>,
    dex_factories: HashMap<DexType, Vec<Address>>,
    min_profit_threshold: U256,
}

impl ArbitrageStrategy {
    pub fn new(config: Arc<Config>) -> Self {
        let mut dex_factories = HashMap::new();
        
        // Initialize known DEX factories
        dex_factories.insert(DexType::UniswapV2, vec![
            "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f".parse().unwrap(),
        ]);
        dex_factories.insert(DexType::SushiSwap, vec![
            "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac".parse().unwrap(),
        ]);
        dex_factories.insert(DexType::PancakeSwap, vec![
            "0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73".parse().unwrap(),
        ]);

        Self {
            config,
            dex_factories,
            min_profit_threshold: U256::from(10).pow(U256::from(17)), // 0.1 ETH
        }
    }

    pub async fn analyze(&self, _tx: &Transaction) -> Vec<MEVOpportunity> {
        let mut opportunities = Vec::new();

        // Extract token addresses from transaction
        let tokens = self.extract_tokens_from_tx(_tx);
        
        for token in tokens {
            // Check triangular arbitrage opportunities
            if let Some(opp) = self.find_triangular_arbitrage(&token).await {
                opportunities.push(opp);
            }
            
            // Check cross-DEX arbitrage
            if let Some(opp) = self.find_cross_dex_arbitrage(&token).await {
                opportunities.push(opp);
            }
        }

        opportunities
    }

    async fn find_triangular_arbitrage(&self, token: &Address) -> Option<MEVOpportunity> {
        // Common triangular paths: WETH -> Token -> USDC -> WETH
        let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap();
        let usdc: Address = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap();
        
        let path = vec![weth, *token, usdc, weth];
        
        // Get pool info for each hop
        let mut pools = Vec::new();
        for i in 0..path.len()-1 {
            if let Some(pool_info) = self.get_pool_info(path[i], path[i+1], DexType::UniswapV2).await {
                pools.push(pool_info);
            } else {
                return None;
            }
        }

        // Calculate potential profit
        let test_amount = U256::from(10).pow(U256::from(18)); // 1 ETH
        let profit = self.calculate_arbitrage_profit(&path, &pools, test_amount);
        
        if profit.profit > self.min_profit_threshold {
            Some(MEVOpportunity {
                id: format!("arb_tri_{}_{}", token, self.get_timestamp()),
                target_tx: Transaction::default(), // Not directly tied to a tx
                strategy_type: StrategyType::Arbitrage(ArbitrageDetails {
                    path: path.clone(),
                    pools: pools.clone(),
                    amount_in: profit.optimal_amount,
                    expected_profit: profit.profit,
                    gas_estimate: U256::from(400000),
                }),
                estimated_profit: profit.profit,
                gas_cost: U256::from(400000) * U256::from(100) * U256::from(10).pow(U256::from(9)),
                priority: 7,
                expiry_block: self.get_current_block().await + 1,
            })
        } else {
            None
        }
    }

    async fn find_cross_dex_arbitrage(&self, token: &Address) -> Option<MEVOpportunity> {
        let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap();
        
        // Get prices across different DEXs
        let mut dex_prices = HashMap::new();
        
        for (dex_type, _) in &self.dex_factories {
            if let Some(pool_info) = self.get_pool_info(weth, *token, *dex_type).await {
                let price = self.calculate_price(&pool_info, true);
                dex_prices.insert(*dex_type, (price, pool_info));
            }
        }

        // Find best arbitrage opportunity
        let mut best_profit = U256::from(0);
        let mut best_opportunity = None;

        for (buy_dex, (buy_price, buy_pool)) in &dex_prices {
            for (sell_dex, (sell_price, sell_pool)) in &dex_prices {
                if buy_dex != sell_dex && sell_price > buy_price {
                    let price_diff_percent: U256 = ((sell_price - buy_price) * 10000) / buy_price;
                    
                    // Need at least 0.3% price difference to be profitable after gas
                    if price_diff_percent > U256::from(30) {
                        let optimal_amount = self.calculate_optimal_arb_amount(
                            buy_pool,
                            sell_pool,
                            price_diff_percent.as_u64(),
                        );
                        
                        let profit = self.simulate_cross_dex_arb(
                            &optimal_amount,
                            buy_pool,
                            sell_pool,
                        );
                        
                        if profit > best_profit {
                            best_profit = profit;
                            best_opportunity = Some((
                                vec![weth, *token, weth],
                                vec![buy_pool.clone(), sell_pool.clone()],
                                optimal_amount,
                            ));
                        }
                    }
                }
            }
        }

        if let Some((path, pools, amount)) = best_opportunity {
            if best_profit > self.min_profit_threshold {
                return Some(MEVOpportunity {
                    id: format!("arb_cross_{}_{}", token, self.get_timestamp()),
                    target_tx: Transaction::default(),
                    strategy_type: StrategyType::Arbitrage(ArbitrageDetails {
                        path,
                        pools,
                        amount_in: amount,
                        expected_profit: best_profit,
                        gas_estimate: U256::from(350000),
                    }),
                    estimated_profit: best_profit,
                    gas_cost: U256::from(350000) * U256::from(100) * U256::from(10).pow(U256::from(9)),
                    priority: 8,
                    expiry_block: self.get_current_block().await + 1,
                });
            }
        }

        None
    }

    fn calculate_arbitrage_profit(
        &self,
        path: &[Address],
        pools: &[PoolInfo],
        test_amount: U256,
    ) -> ArbitrageProfit {
        let mut current_amount = test_amount;
        
        // Simulate swaps through the path
        for (i, pool) in pools.iter().enumerate() {
            let token_in = path[i];
            let _token_out = path[i + 1];
            
            let (amount_out, _, _) = if token_in == pool.token0 {
                uni::get_amount_out(current_amount, pool.reserve0, pool.reserve1)
            } else {
                uni::get_amount_out(current_amount, pool.reserve1, pool.reserve0)
            };
            
            current_amount = amount_out;
        }
        
        let profit = if current_amount > test_amount {
            current_amount - test_amount
        } else {
            U256::from(0)
        };

        // Use binary search to find optimal amount
        let optimal_amount = self.binary_search_optimal_amount(path, pools, profit > U256::from(0));
        
        ArbitrageProfit {
            profit,
            optimal_amount,
        }
    }

    fn binary_search_optimal_amount(
        &self,
        path: &[Address],
        pools: &[PoolInfo],
        profitable: bool,
    ) -> U256 {
        if !profitable {
            return U256::from(0);
        }

        let mut low = U256::from(10).pow(U256::from(16)); // 0.01 ETH
        let mut high = U256::from(100) * U256::from(10).pow(U256::from(18)); // 100 ETH
        let mut best_amount = U256::from(0);
        let mut best_profit = U256::from(0);

        while low <= high {
            let mid = (low + high) / 2;
            let result = self.calculate_arbitrage_profit(path, pools, mid);
            
            if result.profit > best_profit {
                best_profit = result.profit;
                best_amount = mid;
            }

            // Adjust search range
            if result.profit > U256::from(0) {
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }

        best_amount
    }

    fn calculate_optimal_arb_amount(
        &self,
        buy_pool: &PoolInfo,
        sell_pool: &PoolInfo,
        _price_diff_basis_points: u64,
    ) -> U256 {
        // Simplified optimal amount calculation
        // In production, use more sophisticated math
        let max_impact = U256::from(100); // 1% max price impact
        let pool_liquidity = buy_pool.reserve0.min(sell_pool.reserve0);
        
        pool_liquidity * max_impact / 10000
    }

    fn simulate_cross_dex_arb(
        &self,
        amount: &U256,
        buy_pool: &PoolInfo,
        sell_pool: &PoolInfo,
    ) -> U256 {
        // Buy on first DEX
        let (tokens_bought, _, _) = uni::get_amount_out(
            *amount,
            buy_pool.reserve0,
            buy_pool.reserve1,
        );
        
        // Sell on second DEX
        let (eth_received, _, _) = uni::get_amount_out(
            tokens_bought,
            sell_pool.reserve1,
            sell_pool.reserve0,
        );
        
        if eth_received > *amount {
            eth_received - amount
        } else {
            U256::from(0)
        }
    }

    fn calculate_price(&self, pool: &PoolInfo, is_token0_weth: bool) -> U256 {
        if is_token0_weth {
            (pool.reserve0 * U256::from(10).pow(U256::from(18))) / pool.reserve1
        } else {
            (pool.reserve1 * U256::from(10).pow(U256::from(18))) / pool.reserve0
        }
    }

    fn extract_tokens_from_tx(&self, _tx: &Transaction) -> Vec<Address> {
        // Extract token addresses from transaction data
        // This is simplified - in production, decode all relevant calls
        Vec::new()
    }

    async fn get_pool_info(&self, token0: Address, token1: Address, dex: DexType) -> Option<PoolInfo> {
        // Get pool information from chain
        // In production, this should query the actual pool contract
        Some(PoolInfo {
            address: Address::zero(),
            token0,
            token1,
            reserve0: U256::from(1000000) * U256::from(10).pow(U256::from(18)),
            reserve1: U256::from(2000000) * U256::from(10).pow(U256::from(18)),
            fee: 30, // 0.3%
            dex_type: dex,
        })
    }

    async fn get_current_block(&self) -> U64 {
        self.config.http.get_block_number().await.unwrap_or_default()
    }

    fn get_timestamp(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

struct ArbitrageProfit {
    profit: U256,
    optimal_amount: U256,
}
