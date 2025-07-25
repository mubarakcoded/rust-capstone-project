#![allow(unused)]
use bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::{Amount, Network, SignedAmount};
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
    let rpc_connection_string = RPC_URL;
    let rpc_credentials = Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned());

    let main_client = Client::new(rpc_connection_string, rpc_credentials.clone())?;

    let existing_wallets = main_client.list_wallets()?;

    let wallet_names = ["MiningFund", "TradingAccount"];
    for wallet_name in &wallet_names {
        if !existing_wallets.contains(&wallet_name.to_string()) {
            main_client.create_wallet(wallet_name, None, None, None, None)?;
        }
    }

    let mining_wallet_client = Client::new(
        &format!("{rpc_connection_string}/wallet/MiningFund"),
        rpc_credentials.clone(),
    )?;
    let trading_wallet_client = Client::new(
        &format!("{rpc_connection_string}/wallet/TradingAccount"),
        rpc_credentials,
    )?;

    let mining_address = mining_wallet_client
        .get_new_address(Some("Initial Mining Payout"), None)?
        .assume_checked();

    let mut generated_blocks_count = 0;
    let target_balance_btc = 50.0;

    while mining_wallet_client.get_balance(None, None)?
        < Amount::from_btc(target_balance_btc).unwrap()
    {
        mining_wallet_client.generate_to_address(1, &mining_address)?;
        generated_blocks_count += 1;
    }

    println!("Generated {generated_blocks_count} blocks to reach spendable balance");

    let current_mining_balance = mining_wallet_client.get_balance(None, None)?;
    println!(
        "Mining wallet current balance: {} BTC",
        current_mining_balance.to_btc()
    );

    let trading_address = trading_wallet_client
        .get_new_address(Some("Incoming Funds"), None)?
        .assume_checked();

    println!("Mining address: {mining_address}");
    println!("Trading address: {trading_address}");

    let transaction_amount = Amount::from_btc(20.0).unwrap();
    let transaction_id = mining_wallet_client.send_to_address(
        &trading_address,
        transaction_amount,
        None,
        None,
        None,
        None,
        Some(6),
        None,
    )?;

    println!("Transaction broadcasted with ID: {transaction_id}");

    let transaction_in_mempool = mining_wallet_client.get_mempool_entry(&transaction_id)?;
    println!("Transaction status in mempool: {transaction_in_mempool:?}");

    let confirmation_blocks = mining_wallet_client.generate_to_address(1, &mining_address)?;
    let confirmed_block_hash = confirmation_blocks[0];

    let transaction_details = mining_wallet_client.get_transaction(&transaction_id, Some(true))?;
    let raw_transaction =
        mining_wallet_client.get_raw_transaction(&transaction_id, Some(&confirmed_block_hash))?;
    let decoded_transaction =
        mining_wallet_client.decode_raw_transaction(&raw_transaction, None)?;

    let mut sender_input_address = String::new();
    let mut sender_input_value = 0.0;
    let receiver_output_address = trading_address.to_string();
    let mut receiver_output_value = 0.0;
    let mut sender_change_address = String::new();
    let mut sender_change_value = 0.0;
    let transaction_fee = transaction_details
        .fee
        .unwrap_or_else(|| SignedAmount::from_sat(0))
        .to_btc()
        .abs();
    let block_height_confirmed = transaction_details.info.blockheight.unwrap_or(0);
    let block_hash_confirmed = transaction_details.info.blockhash;

    if let Some(first_input) = decoded_transaction.vin.first() {
        if let Some(previous_txid) = first_input.txid {
            let previous_tx_info =
                mining_wallet_client.get_transaction(&previous_txid, Some(true))?;
            let raw_previous_tx = previous_tx_info.hex;
            let previous_decoded_tx =
                mining_wallet_client.decode_raw_transaction(&raw_previous_tx, None)?;

            if let Some(output_index) = first_input.vout {
                if let Some(output_detail) = previous_decoded_tx.vout.get(output_index as usize) {
                    sender_input_address = output_detail
                        .script_pub_key
                        .address
                        .as_ref()
                        .and_then(|addr| addr.clone().require_network(Network::Regtest).ok())
                        .map(|addr| addr.to_string())
                        .unwrap_or_default();
                    sender_input_value = output_detail.value.to_btc();
                }
            }
        }
    }

    for output in &decoded_transaction.vout {
        let output_address = output
            .script_pub_key
            .address
            .as_ref()
            .and_then(|addr| addr.clone().require_network(Network::Regtest).ok())
            .map(|addr| addr.to_string())
            .unwrap_or_default();
        let output_value = output.value.to_btc();

        if output_address == receiver_output_address {
            receiver_output_value = output_value;
        } else if !output_address.is_empty() && output_address != receiver_output_address {
            sender_change_address = output_address;
            sender_change_value = output_value;
        }
    }

    let mut output_file = File::create("out.txt")?;
    writeln!(output_file, "{transaction_id}")?;
    writeln!(output_file, "{sender_input_address}")?;
    writeln!(output_file, "{sender_input_value}")?;
    writeln!(output_file, "{receiver_output_address}")?;
    writeln!(output_file, "{receiver_output_value}")?;
    writeln!(output_file, "{sender_change_address}")?;
    writeln!(output_file, "{sender_change_value}")?;
    writeln!(output_file, "{transaction_fee}")?;
    writeln!(output_file, "{block_height_confirmed}")?;
    writeln!(output_file, "{}", block_hash_confirmed.unwrap())?;

    Ok(())
}
