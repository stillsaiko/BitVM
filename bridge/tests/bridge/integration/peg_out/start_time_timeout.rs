use std::time::Duration;
use tokio::time::sleep;

use bitcoin::{Address, Amount, OutPoint};
use bridge::{
    connectors::base::TaprootConnector,
    graphs::base::{DUST_AMOUNT, FEE_AMOUNT, INITIAL_AMOUNT, MESSAGE_COMMITMENT_FEE_AMOUNT},
    scripts::generate_pay_to_pubkey_script_address,
    transactions::{
        base::{BaseTransaction, Input},
        pre_signed_musig2::PreSignedMusig2Transaction,
        start_time_timeout::StartTimeTimeoutTransaction,
    },
};

use crate::bridge::{
    faucet::{Faucet, FaucetType},
    helper::verify_funding_inputs,
    integration::peg_out::utils::create_and_mine_kick_off_1_tx,
    setup::setup_test,
};

#[tokio::test]
async fn test_start_time_timeout_success() {
    let config = setup_test().await;
    let faucet = Faucet::new(FaucetType::EsploraRegtest);

    // verify funding inputs
    let mut funding_inputs: Vec<(&Address, Amount)> = vec![];
    let kick_off_1_input_amount = Amount::from_sat(
        INITIAL_AMOUNT + 2 * DUST_AMOUNT + 2 * MESSAGE_COMMITMENT_FEE_AMOUNT + FEE_AMOUNT,
    );
    let kick_off_1_funding_utxo_address = config.connector_6.generate_taproot_address();
    funding_inputs.push((&kick_off_1_funding_utxo_address, kick_off_1_input_amount));
    faucet
        .fund_inputs(&config.client_0, &funding_inputs)
        .await
        .wait()
        .await;

    verify_funding_inputs(&config.client_0, &funding_inputs).await;

    // kick-off 1
    let (kick_off_1_tx, kick_off_1_txid) = create_and_mine_kick_off_1_tx(
        &config.client_0,
        &config.operator_context,
        &kick_off_1_funding_utxo_address,
        &config.connector_1,
        &config.connector_2,
        &config.connector_6,
        kick_off_1_input_amount,
        &config.commitment_secrets,
    )
    .await;

    // start time timeout
    let vout = 2; // connector 2
    let start_time_timeout_input_0 = Input {
        outpoint: OutPoint {
            txid: kick_off_1_txid,
            vout,
        },
        amount: kick_off_1_tx.output[vout as usize].value,
    };
    let vout = 1; // connector 1
    let start_time_timeout_input_1 = Input {
        outpoint: OutPoint {
            txid: kick_off_1_txid,
            vout,
        },
        amount: kick_off_1_tx.output[vout as usize].value,
    };
    let mut start_time_timeout = StartTimeTimeoutTransaction::new(
        &config.operator_context,
        &config.connector_1,
        &config.connector_2,
        start_time_timeout_input_0,
        start_time_timeout_input_1,
    );

    let secret_nonces_0 = start_time_timeout.push_nonces(&config.verifier_0_context);
    let secret_nonces_1 = start_time_timeout.push_nonces(&config.verifier_1_context);

    start_time_timeout.pre_sign(
        &config.verifier_0_context,
        &config.connector_1,
        &config.connector_2,
        &secret_nonces_0,
    );
    start_time_timeout.pre_sign(
        &config.verifier_1_context,
        &config.connector_1,
        &config.connector_2,
        &secret_nonces_1,
    );

    let reward_address = generate_pay_to_pubkey_script_address(
        config.withdrawer_context.network,
        &config.withdrawer_context.withdrawer_public_key,
    );
    start_time_timeout.add_output(reward_address.script_pubkey());

    let start_time_timeout_tx = start_time_timeout.finalize();
    let start_time_timeout_txid = start_time_timeout_tx.compute_txid();

    // mine start time timeout
    let start_time_timeout_wait_timeout = Duration::from_secs(20);
    println!(
        "Waiting \x1b[37;41m{:?}\x1b[0m before broadcasting start time timeout tx...",
        start_time_timeout_wait_timeout
    );
    sleep(start_time_timeout_wait_timeout).await;
    let start_time_timeout_result = config
        .client_0
        .esplora
        .broadcast(&start_time_timeout_tx)
        .await;
    println!("Start time timeout result: {:?}", start_time_timeout_result);
    assert!(start_time_timeout_result.is_ok());

    // reward balance
    let reward_utxos = config
        .client_0
        .esplora
        .get_address_utxo(reward_address)
        .await
        .unwrap();
    let reward_utxo = reward_utxos
        .clone()
        .into_iter()
        .find(|x| x.txid == start_time_timeout_txid);

    // assert
    assert!(reward_utxo.is_some());
    assert_eq!(
        reward_utxo.unwrap().value,
        start_time_timeout_tx.output[1].value
    );
}
