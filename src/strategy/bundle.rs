use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::utils::keccak256;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use super::types::*;

#[derive(Debug)]
pub struct BundleBuilder {
    provider: Arc<SignerMiddleware<Provider<Http>, Wallet<k256::ecdsa::SigningKey>>>,
    flashbots_signer: Wallet<k256::ecdsa::SigningKey>,
    flashbots_relay: String,
}

impl BundleBuilder {
    pub fn new(provider: Arc<SignerMiddleware<Provider<Http>, Wallet<k256::ecdsa::SigningKey>>>) -> Self {
        // Create a separate signer for Flashbots authentication
        let flashbots_signer = Wallet::new(&mut rand::thread_rng());
        
        Self {
            provider,
            flashbots_signer,
            flashbots_relay: "https://relay.flashbots.net".to_string(),
        }
    }

    pub async fn build_sandwich_bundle(
        &self,
        victim_tx: &Transaction,
        details: &SandwichDetails,
        _estimated_profit: U256,
    ) -> Result<Bundle, Box<dyn std::error::Error + Send + Sync>> {
        let block_number = self.provider.get_block_number().await?;
        let signer_address = self.provider.address();
        
        // Prepare bundle transactions
        let mut bundle_txs = Vec::new();
        
        // 1. Frontrun transaction
        let frontrun_signed = self.sign_transaction(details.frontrun_tx.clone()).await?;
        bundle_txs.push(BundleTransaction {
            signer: signer_address,
            tx: frontrun_signed,
            can_revert: false,
        });
        
        // 2. Victim transaction (convert to TypedTransaction)
        let mut victim_typed = TypedTransaction::default();
        victim_typed.set_from(victim_tx.from)
            .set_to(victim_tx.to.unwrap())
            .set_value(victim_tx.value)
            .set_data(victim_tx.input.clone())
            .set_gas(victim_tx.gas)
            .set_nonce(victim_tx.nonce);
        
        if let Some(gas_price) = victim_tx.gas_price {
            victim_typed.set_gas_price(gas_price);
        }
        
        bundle_txs.push(BundleTransaction {
            signer: victim_tx.from,
            tx: victim_typed,
            can_revert: true,
        });
        
        // 3. Backrun transaction
        let backrun_signed = self.sign_transaction(details.backrun_tx.clone()).await?;
        bundle_txs.push(BundleTransaction {
            signer: signer_address,
            tx: backrun_signed,
            can_revert: false,
        });
        
        Ok(Bundle {
            txs: bundle_txs,
            block_number: block_number + 1,
        })
    }

    pub async fn build_arbitrage_tx(
        &self,
        details: &ArbitrageDetails,
        _estimated_profit: U256,
    ) -> Result<TypedTransaction, Box<dyn std::error::Error + Send + Sync>> {
        // Build an optimized arbitrage transaction
        let mut tx = TypedTransaction::default();
        
        // Set transaction parameters
        tx.set_from(self.provider.address())
            .set_to(details.pools[0].address) // First pool in path
            .set_gas(details.gas_estimate)
            .set_value(if details.path[0] == self.get_weth_address() { details.amount_in } else { U256::from(0) })
            .set_data(self.encode_arbitrage_data(details)?);
        
        // Set competitive gas price
        let gas_price = self.calculate_optimal_gas_price(_estimated_profit, details.gas_estimate).await?;
        tx.set_gas_price(gas_price);
        
        Ok(tx)
    }

    pub async fn send_bundle(&self, bundle: Bundle) -> Result<TxHash, Box<dyn std::error::Error + Send + Sync>> {
        // Serialize bundle for Flashbots
        let bundle_body = self.serialize_bundle(&bundle).await?;
        
        // Sign the bundle with Flashbots signer
        let signature = self.sign_bundle_body(&bundle_body)?;
        
        // Send to Flashbots relay
        let response = self.submit_to_flashbots(bundle_body, signature, bundle.block_number).await?;
        
        // Parse bundle hash from response
        if let Some(result) = response.result {
            Ok(result.bundle_hash.parse()?)
        } else {
            Err("No bundle hash in response".into())
        }
    }

    async fn sign_transaction(&self, mut tx: TypedTransaction) -> Result<TypedTransaction, Box<dyn std::error::Error + Send + Sync>> {
        // Fill transaction details
        self.provider.fill_transaction(&mut tx, None).await?;
        
        Ok(tx)
    }

    fn encode_arbitrage_data(&self, _details: &ArbitrageDetails) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        // Encode the arbitrage swap data
        // In production, this should encode proper router calls
        Ok(Bytes::default())
    }

    async fn calculate_optimal_gas_price(
        &self,
        profit: U256,
        gas_estimate: U256,
    ) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
        // Get base fee and priority fee
        let base_fee = self.provider.get_block(BlockNumber::Latest)
            .await?
            .unwrap()
            .base_fee_per_gas
            .unwrap_or_default();
        
        // Calculate maximum viable gas price based on profit
        let max_gas_price = profit / gas_estimate;
        
        // Use 80% of profit for gas to ensure profitability
        let target_gas_price: U256 = max_gas_price * 80 / 100;
        
        // Ensure we pay at least base fee + priority
        let min_gas_price = base_fee + U256::from(2_000_000_000); // 2 gwei priority
        
        Ok(target_gas_price.max(min_gas_price))
    }

    async fn serialize_bundle(&self, bundle: &Bundle) -> Result<FlashbotsBundle, Box<dyn std::error::Error + Send + Sync>> {
        let mut signed_transactions = Vec::new();
        
        for bundle_tx in &bundle.txs {
            // Get raw signed transaction
            let raw_tx = self.provider.signer().sign_transaction(&bundle_tx.tx).await?;
            signed_transactions.push(format!("0x{}", hex::encode(raw_tx.to_vec())));
        }
        
        Ok(FlashbotsBundle {
            signed_transactions,
            block_number: format!("0x{:x}", bundle.block_number.as_u64()),
            min_timestamp: None,
            max_timestamp: None,
            reverting_tx_hashes: Vec::new(),
        })
    }

    fn sign_bundle_body(&self, bundle: &FlashbotsBundle) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Create EIP-191 message
        let message = serde_json::to_string(bundle)?;
        let message_hash = keccak256(message.as_bytes());
        
        // Sign with Flashbots signer
        let signature = self.flashbots_signer.sign_hash(H256::from(message_hash))?;
        
        Ok(format!("0x{}", hex::encode(signature.to_vec())))
    }

    async fn submit_to_flashbots(
        &self,
        bundle: FlashbotsBundle,
        signature: String,
        _target_block: U64,
    ) -> Result<FlashbotsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let client = reqwest::Client::new();
        
        let request_body = FlashbotsRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_sendBundle".to_string(),
            params: vec![bundle],
            id: 1,
        };
        
        let response = client
            .post(&self.flashbots_relay)
            .header("X-Flashbots-Signature", format!("{}:{}", self.flashbots_signer.address(), signature))
            .json(&request_body)
            .send()
            .await?;
        
        let response_body: FlashbotsResponse = response.json().await?;
        
        if let Some(error) = response_body.error {
            return Err(format!("Flashbots error: {:?}", error).into());
        }
        
        Ok(response_body)
    }

    fn get_weth_address(&self) -> Address {
        "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FlashbotsBundle {
    #[serde(rename = "txs")]
    signed_transactions: Vec<String>,
    #[serde(rename = "blockNumber")]
    block_number: String,
    #[serde(rename = "minTimestamp", skip_serializing_if = "Option::is_none")]
    min_timestamp: Option<u64>,
    #[serde(rename = "maxTimestamp", skip_serializing_if = "Option::is_none")]
    max_timestamp: Option<u64>,
    #[serde(rename = "revertingTxHashes")]
    reverting_tx_hashes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FlashbotsRequest {
    jsonrpc: String,
    method: String,
    params: Vec<FlashbotsBundle>,
    id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct FlashbotsResponse {
    jsonrpc: String,
    id: u64,
    result: Option<FlashbotsResult>,
    error: Option<FlashbotsError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FlashbotsResult {
    #[serde(rename = "bundleHash")]
    bundle_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FlashbotsError {
    code: i32,
    message: String,
} 