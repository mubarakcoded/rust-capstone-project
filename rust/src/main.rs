#![allow(unused)]
use bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::Amount;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::io::Write;

// Node access params
const RPC_URL: &str = "http://127.0.0.1:18443"; // Default regtest RPC port
const RPC_USER: &str = "alice";
const RPC_PASS: &str = "password";

// You can use calls not provided in RPC lib API using the generic `call` function.
// An example of using the `send` RPC call, which doesn't have exposed API.
// You can also use serde_json `Deserialize` derivation to capture the returned json result.
fn send(rpc: &Client, addr: &str) -> bitcoincore_rpc::Result<String> {
    let args = [
        json!([{addr : 100 }]), // recipient address
        json!(null),            // conf target
        json!(null),            // estimate mode
        json!(null),            // fee rate in sats/vb
        json!(null),            // Empty option object
    ];

    #[derive(Deserialize)]
    struct SendResult {
        complete: bool,
        txid: String,
    }
    let send_result = rpc.call::<SendResult>("send", &args)?;
    assert!(send_result.complete);
    Ok(send_result.txid)
}

fn main() -> bitcoincore_rpc::Result<()> {
    // First, connect to the Bitcoin Core RPC server
    let rpc = Client::new(
        RPC_URL,
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Just checking if we're connected properly
    let blockchain_info = rpc.get_blockchain_info()?;
    println!("Blockchain Info: {:?}", blockchain_info);

    // Step 1: Set up the Miner and Trader wallets
    println!("\n=== Setting up wallets ===");

    // Check which wallets are currently loaded
    let loaded_wallets = rpc.list_wallets()?;

    // Try to create the Miner wallet, or load it if it already exists
    if !loaded_wallets.contains(&"Miner".to_string()) {
        match rpc.create_wallet("Miner", None, None, None, None) {
            Ok(_) => println!("Created Miner wallet"),
            Err(e) => {
                // Might already exist on disk, so just try loading it
                match rpc.load_wallet("Miner") {
                    Ok(_) => println!("Loaded existing Miner wallet"),
                    Err(_) => return Err(e),
                }
            }
        }
    } else {
        println!("Miner wallet already loaded");
    }

    // Same thing for the Trader wallet
    if !loaded_wallets.contains(&"Trader".to_string()) {
        match rpc.create_wallet("Trader", None, None, None, None) {
            Ok(_) => println!("Created Trader wallet"),
            Err(e) => match rpc.load_wallet("Trader") {
                Ok(_) => println!("Loaded existing Trader wallet"),
                Err(_) => return Err(e),
            },
        }
    } else {
        println!("Trader wallet already loaded");
    }

    // Now get a dedicated RPC client for the Miner wallet
    let miner_rpc = Client::new(
        &format!("{}/wallet/Miner", RPC_URL),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Step 2: Create a mining address and mine blocks
    println!("\n=== Generating mining address ===");

    // Get a new address from the Miner wallet for receiving mining rewards
    let mining_address = miner_rpc
        .get_new_address(Some("Mining Reward"), None)?
        .assume_checked();
    println!("Mining address: {}", mining_address);

    println!("\n=== Mining blocks to generate spendable balance ===");

    // Here's the thing: we need to mine 101 blocks to get spendable coins
    // Why? Because in Bitcoin, coinbase rewards (mining rewards) need 100 confirmations
    // before you can spend them. It's a safety mechanism to prevent issues if the chain
    // gets reorganized. So mining 101 blocks means the first block's reward is now mature.
    let blocks_to_mine = 101;
    let block_hashes = miner_rpc.generate_to_address(blocks_to_mine, &mining_address)?;
    println!("Mined {} blocks", blocks_to_mine);

    // Let's check how much BTC we have now
    let miner_balance = miner_rpc.get_balance(None, None)?;
    println!("Miner wallet balance: {} BTC", miner_balance.to_btc());

    // Step 3: Set up the Trader wallet and get a receiving address
    println!("\n=== Setting up Trader wallet ===");
    let trader_rpc = Client::new(
        &format!("{}/wallet/Trader", RPC_URL),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Generate an address where the Trader will receive funds
    let trader_address = trader_rpc
        .get_new_address(Some("Received"), None)?
        .assume_checked();
    println!("Trader receiving address: {}", trader_address);

    // Step 4: Send 20 BTC from Miner to Trader
    println!("\n=== Sending 20 BTC from Miner to Trader ===");
    let send_amount = Amount::from_btc(20.0)?;
    let txid = miner_rpc.send_to_address(
        &trader_address,
        send_amount,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;
    println!("Transaction ID: {}", txid);

    // Step 5: Check that the transaction is in the mempool (unconfirmed)
    println!("\n=== Checking mempool ===");

    // Define structs to deserialize the mempool entry data
    #[derive(Deserialize, Debug)]
    struct MempoolEntry {
        vsize: u64,
        fees: MempoolFees,
    }

    #[derive(Deserialize, Debug)]
    struct MempoolFees {
        base: f64,
    }

    let mempool_entry: MempoolEntry = rpc.call("getmempoolentry", &[json!(txid.to_string())])?;
    println!("Mempool entry: {:?}", mempool_entry);

    // Step 6: Mine a block to confirm the transaction
    println!("\n=== Mining 1 block to confirm transaction ===");
    let confirm_blocks = miner_rpc.generate_to_address(1, &mining_address)?;
    let confirm_block_hash = confirm_blocks[0];
    println!("Confirmed in block: {}", confirm_block_hash);

    // Step 7: Extract all the transaction details we need
    println!("\n=== Extracting transaction details ===");

    // Figure out what block height this transaction is in
    let block_info = rpc.get_block_info(&confirm_block_hash)?;
    let block_height = block_info.height;

    // Get the full transaction with verbose details
    let tx_verbose: serde_json::Value =
        rpc.call("getrawtransaction", &[json!(txid.to_string()), json!(true)])?;

    // Parse the input (where the funds came from)
    let vin = &tx_verbose["vin"][0];
    let prev_txid = vin["txid"].as_str().unwrap();
    let prev_vout = vin["vout"].as_u64().unwrap();

    // Look up the previous transaction to get the input address and amount
    let prev_tx: serde_json::Value =
        rpc.call("getrawtransaction", &[json!(prev_txid), json!(true)])?;
    let prev_output = &prev_tx["vout"][prev_vout as usize];
    let input_amount = prev_output["value"].as_f64().unwrap();
    let input_address = prev_output["scriptPubKey"]["address"].as_str().unwrap();

    // Parse the outputs (where the funds went)
    let vout = &tx_verbose["vout"];

    // We need to figure out which output went to the Trader and which is change back to Miner
    let trader_address_str = trader_address.to_string();
    let mut trader_output_address = String::new();
    let mut trader_output_amount = 0.0;
    let mut change_address = String::new();
    let mut change_amount = 0.0;

    for output in vout.as_array().unwrap() {
        let amount = output["value"].as_f64().unwrap();
        let addr = output["scriptPubKey"]["address"].as_str().unwrap_or("");

        if addr == trader_address_str {
            // This is the payment to Trader
            trader_output_address = addr.to_string();
            trader_output_amount = amount;
        } else if !addr.is_empty() {
            // This must be the change going back to Miner
            change_address = addr.to_string();
            change_amount = amount;
        }
    }

    // Calculate the fee (input minus outputs)
    let tx_fee = input_amount - trader_output_amount - change_amount;

    // Step 8: Write everything to the output file
    println!("\n=== Writing output to file ===");

    // Write to the project root, not the rust/ directory
    let output_path = "../out.txt";
    let mut file = File::create(output_path)?;

    writeln!(file, "{}", txid)?;
    writeln!(file, "{}", input_address)?;
    writeln!(file, "{}", input_amount)?;
    writeln!(file, "{}", trader_output_address)?;
    writeln!(file, "{}", trader_output_amount)?;
    writeln!(file, "{}", change_address)?;
    writeln!(file, "{}", change_amount)?;
    writeln!(file, "{}", tx_fee)?;
    writeln!(file, "{}", block_height)?;
    writeln!(file, "{}", confirm_block_hash)?;

    println!("Output written to {}", output_path);
    println!("\n=== Transaction Summary ===");
    println!("Transaction ID: {}", txid);
    println!("Block Height: {}", block_height);
    println!("Block Hash: {}", confirm_block_hash);
    println!("Input Address (Miner): {}", input_address);
    println!("Input Amount: {} BTC", input_amount);
    println!("Output Address (Trader): {}", trader_output_address);
    println!("Output Amount: {} BTC", trader_output_amount);
    println!("Change Address (Miner): {}", change_address);
    println!("Change Amount: {} BTC", change_amount);
    println!("Transaction Fee: {} BTC", tx_fee);

    Ok(())
}
