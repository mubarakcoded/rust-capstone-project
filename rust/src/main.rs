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
    let rpc_root = Client::new(
        RPC_URL,
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    let wallets = rpc_root.list_wallets()?;
    if !wallets.contains(&"Miner".to_string()) {
        rpc_root.create_wallet("Miner", None, None, None, None)?;
    }
    if !wallets.contains(&"Trader".to_string()) {
        rpc_root.create_wallet("Trader", None, None, None, None)?;
    }

    let miner_rpc = Client::new(
        &format!("{RPC_URL}/wallet/Miner"),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;
    let trader_rpc = Client::new(
        &format!("{RPC_URL}/wallet/Trader"),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    let miner_address = miner_rpc
        .get_new_address(Some("Mining Reward"), None)?
        .assume_checked();

    let mut blocks_mined = 0;
    while miner_rpc.get_balance(None, None)? == Amount::ZERO {
        miner_rpc.generate_to_address(1, &miner_address)?;
        blocks_mined += 1;
    }

    while miner_rpc.get_balance(None, None)? < Amount::from_btc(50.0).unwrap() {
        miner_rpc.generate_to_address(1, &miner_address)?;
        blocks_mined += 1;
    }

    println!("Mined {blocks_mined} blocks to generate spendable balance");

    let miner_balance = miner_rpc.get_balance(None, None)?;
    println!("Miner wallet balance: {} BTC", miner_balance.to_btc());

    let trader_address = trader_rpc
        .get_new_address(Some("Received"), None)?
        .assume_checked();

    println!("Miner address: {miner_address}");
    println!("Trader address: {trader_address}");

    let txid = miner_rpc.send_to_address(
        &trader_address,
        Amount::from_btc(20.0).unwrap(),
        None,
        None,
        None,
        None,
        Some(6),
        None,
    )?;

    println!("Transaction sent with txid: {txid}");

    let mempool = miner_rpc.get_mempool_entry(&txid)?;
    println!("Transaction in mempool: {mempool:?}");

    let block_hashes = miner_rpc.generate_to_address(1, &miner_address)?;
    let confirmed_block_hash = block_hashes[0];

    let tx = miner_rpc.get_transaction(&txid, Some(true))?;
    let raw_tx = miner_rpc.get_raw_transaction(&txid, Some(&confirmed_block_hash))?;
    let decoded_tx = miner_rpc.decode_raw_transaction(&raw_tx, None)?;

    let mut miner_input_address = String::new();
    let mut miner_input_amount = 0.0;
    let trader_input_address = trader_address.to_string();
    let mut trader_input_amount = 0.0;
    let mut miner_change_address = String::new();
    let mut miner_change_amount = 0.0;
    let fee = tx
        .fee
        .unwrap_or_else(|| SignedAmount::from_sat(0))
        .to_btc()
        .abs();
    let block_height = tx.info.blockheight.unwrap_or(0);
    let block_hash = tx.info.blockhash;

    // Extract input details (from the previous transaction output being spent)
    if !decoded_tx.vin.is_empty() {
        let vin = &decoded_tx.vin[0];
        if let Some(prev_txid) = vin.txid {
            let prev_tx_info = miner_rpc.get_transaction(&prev_txid, Some(true))?;
            let raw_prev_tx = prev_tx_info.hex;
            let prev_decoded = miner_rpc.decode_raw_transaction(&raw_prev_tx, None)?;

            if let Some(vout_idx) = vin.vout {
                if let Some(vout) = prev_decoded.vout.get(vout_idx as usize) {
                    miner_input_address = vout
                        .script_pub_key
                        .address
                        .as_ref()
                        .and_then(|addr| addr.clone().require_network(Network::Regtest).ok())
                        .map(|addr| addr.to_string())
                        .unwrap_or_default();
                    miner_input_amount = vout.value.to_btc();
                }
            }
        }
    }

    // Extract output details from the current transaction
    for vout in decoded_tx.vout.iter() {
        let address = vout
            .script_pub_key
            .address
            .as_ref()
            .and_then(|addr| addr.clone().require_network(Network::Regtest).ok())
            .map(|addr| addr.to_string())
            .unwrap_or_default();
        let amount = vout.value.to_btc();

        if address == trader_address.to_string() {
            trader_input_amount = amount;
        } else if !address.is_empty() && address != trader_address.to_string() {
            miner_change_address = address;
            miner_change_amount = amount;
        }
    }

    // Write to out.txt in the specified format
    let mut file = File::create("../out.txt")?;
    writeln!(file, "{txid}")?;
    writeln!(file, "{miner_input_address}")?;
    writeln!(file, "{miner_input_amount}")?;
    writeln!(file, "{trader_input_address}")?;
    writeln!(file, "{trader_input_amount}")?;
    writeln!(file, "{miner_change_address}")?;
    writeln!(file, "{miner_change_amount}")?;
    writeln!(file, "{fee}")?;
    writeln!(file, "{block_height}")?;
    writeln!(file, "{}", block_hash.unwrap())?;

    Ok(())
}
