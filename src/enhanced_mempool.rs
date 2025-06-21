use ethers::types::transaction::eip2718::TypedTransaction;

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;

use ethers::{
    providers::{Middleware, Provider, StreamExt, TransactionStream, Ws},
    types::Transaction,
};

use crate::strategy::{StrategyManager, AdvancedMEVFeatures};
use crate::alert::alert;

pub async fn enhanced_mempool_monitor(
    ws_provider: Arc<Provider<Ws>>,
    strategy_manager: Arc<StrategyManager>,
) {
    // Initialize advanced features
    let advanced_features = Arc::new(AdvancedMEVFeatures::new(strategy_manager.config().clone()));
    
    // Track processed transactions
    let processed_txs = Arc::new(Mutex::new(HashMap::new()));
    
    // Subscribe to pending transactions
    let tx_hash_stream = ws_provider.subscribe_pending_txs().await.unwrap();
    let mut tx_stream = TransactionStream::new(&ws_provider, tx_hash_stream, 512); // Increased buffer
    
    println!("🚀 Enhanced MEV Bot Active - Multi-Strategy Mode");
    println!("📊 Strategies: Sandwich, Arbitrage, JIT, Backrun, Statistical Arb");
    println!("----------------------------------------------");
    
    while let Some(maybe_tx) = tx_stream.next().await {
        if let Ok(tx) = maybe_tx {
            // Skip if already processed
            let mut processed = processed_txs.lock().await;
            if processed.contains_key(&tx.hash) {
                continue;
            }
            processed.insert(tx.hash, true);
            
            // Process transaction with multiple strategies
            let strategy_manager_clone = strategy_manager.clone();
            let advanced_features_clone = advanced_features.clone();
            let ws_provider_clone = ws_provider.clone();
            
            tokio::spawn(async move {
                analyze_with_all_strategies(
                    tx,
                    strategy_manager_clone,
                    advanced_features_clone,
                    ws_provider_clone
                ).await;
            });
        }
    }
}

async fn analyze_with_all_strategies(
    tx: Transaction,
    strategy_manager: Arc<StrategyManager>,
    advanced_features: Arc<AdvancedMEVFeatures>,
    ws_provider: Arc<Provider<Ws>>,
) {
    let mut all_opportunities = Vec::new();
    
    // 1. Traditional sandwich & arbitrage
    let basic_opps = strategy_manager.analyze_transaction(&tx).await;
    all_opportunities.extend(basic_opps);
    
    // 2. JIT liquidity opportunities
    if let Some(jit_opp) = advanced_features.find_jit_opportunities(&tx).await {
        println!("💧 JIT Opportunity: {} ETH liquidity, {} ETH fees",
            ethers::utils::format_ether(jit_opp.liquidity_amount),
            ethers::utils::format_ether(jit_opp.expected_fees)
        );
    }
    
    // 3. Backrun opportunities
    let backrun_opps = advanced_features.find_backrun_opportunities(&tx).await;
    for backrun in backrun_opps {
        println!("🎯 Backrun Opportunity: {:?} - {} ETH profit",
            backrun.strategy,
            ethers::utils::format_ether(backrun.expected_profit)
        );
    }
    
    // 4. Multi-DEX arbitrage (check periodically, not on every tx)
    static mut LAST_ARB_CHECK: u64 = 0;
    unsafe {
        if LAST_ARB_CHECK % 100 == 0 {
            let arb_paths = advanced_features.find_multi_dex_arbitrage(tx.from).await;
            for path in arb_paths.iter().take(3) {
                println!("🔄 Arbitrage Path: {} hops, {} ETH profit",
                    path.path.len() - 1,
                    ethers::utils::format_ether(path.expected_profit)
                );
            }
        }
        LAST_ARB_CHECK += 1;
    }
    
    // Execute best opportunity
    if !all_opportunities.is_empty() {
        all_opportunities.sort_by(|a, b| {
            b.estimated_profit.saturating_sub(b.gas_cost)
                .cmp(&a.estimated_profit.saturating_sub(a.gas_cost))
        });
        
        if let Some(best_opp) = all_opportunities.first() {
            execute_opportunity(best_opp, &strategy_manager, &ws_provider).await;
        }
    }
}

async fn execute_opportunity(
    opportunity: &crate::strategy::MEVOpportunity,
    strategy_manager: &Arc<StrategyManager>,
    ws_provider: &Arc<Provider<Ws>>,
) {
    let net_profit = opportunity.estimated_profit.saturating_sub(opportunity.gas_cost);
    
    println!("\n💎 Executing MEV Opportunity:");
    println!("   Type: {:?}", opportunity.strategy_type);
    println!("   Gross Profit: {} ETH", ethers::utils::format_ether(opportunity.estimated_profit));
    println!("   Gas Cost: {} ETH", ethers::utils::format_ether(opportunity.gas_cost));
    println!("   Net Profit: {} ETH", ethers::utils::format_ether(net_profit));
    
    match strategy_manager.execute_opportunity(opportunity).await {
        Ok(tx_hash) => {
            println!("✅ Success! Bundle: {}", tx_hash);
            
            let current_block = ws_provider.get_block_number().await.unwrap_or_default();
            let msg = format!(
                "💰 MEV Executed!\nType: {:?}\nNet Profit: {} ETH\nTx: {}",
                opportunity.strategy_type,
                ethers::utils::format_ether(net_profit),
                tx_hash
            );
            alert(&msg, &current_block.as_u64()).await;
        },
        Err(e) => {
            println!("❌ Execution failed: {}", e);
        }
    }
}
