# Running the Ethereum MEV Bot

This document explains how to configure and launch the MEV bot included in this repository. The bot requires a Rust toolchain and an Ethereum node endpoint. It will not execute trades unless the environment is correctly configured.

## 1. Install prerequisites

- Rust 1.65 or newer: <https://www.rust-lang.org/tools/install>
- A WebSocket enabled Ethereum RPC endpoint (Infura, Alchemy, or your own node)
- A funded Ethereum account and its private key (for signing transactions)

Optional tools:
- Docker (for containerized execution)
- Python 3 if you plan to use auxiliary scripts

## 2. Configure environment variables

Create a `.env` file in the project root by copying the example:

```bash
cp .env.example .env
```

Edit `.env` and fill in the following values:

```ini
PRIVATE_KEY=0xYourPrivateKeyHere           # Used to sign transactions
NETWORK_RPC=https://mainnet.example.com    # HTTP RPC endpoint
NETWORK_WSS=wss://mainnet.example.com/ws   # WebSocket endpoint
DISCORD_WEBHOOK=https://discord.com/api/webhooks/your_webhook
```

For testing purposes you can use the values in `test_config.env`. **Never use real funds until you have validated the bot in a safe environment.**

## 3. Build the project

Compile the bot in release mode:

```bash
cargo build --release
```

## 4. Running the bot

Development mode:

```bash
cargo run
```

Production mode (optimized binary):

```bash
cargo run --release
```

The bot will connect to your Ethereum node, monitor the mempool and attempt to execute profitable opportunities. Logs will be printed to the console. If a Discord webhook is configured, alerts will be posted there as well.

## 5. Safety tips

- Begin on a testnet or with small amounts.
- Monitor gas prices and adjust `MAX_GAS_PRICE_GWEI` in your environment file.
- Review transaction logs regularly.
- Stop the bot immediately if you notice unexpected behavior.

For advanced configuration and strategy details, see `PRODUCTION_SETUP.md` and the comments in the source code.
