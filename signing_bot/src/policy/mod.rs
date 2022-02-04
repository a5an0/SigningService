use std::env;
use std::str::FromStr;

use aws_config::Config;
use aws_sdk_dynamodb::model::AttributeValue;
use bdk::bitcoin::util::psbt::PartiallySignedTransaction;
use bdk::database::BatchDatabase;
use bdk::Wallet;
use log::info;

use crate::policy::andonpolicy::AndonPolicy;
use crate::policy::valuepolicy::ValuePolicy;

mod andonpolicy;
mod valuepolicy;

#[derive(Debug)]
pub struct PolicyConfig {
    wallet_name: String,
    max_spend_per_tx: u64,
    all_tx_halted: bool,
}

trait Policy {
    fn check_transaction(&self, psbt: &PartiallySignedTransaction) -> Result<(), String>;
}

pub struct PolicySet<'a> {
    policies: Vec<Box<dyn Policy + 'a>>,
}

impl<'a> PolicySet<'a> {
    pub fn new<B, D>(wallet: &'a Wallet<B, D>, policy_config: &PolicyConfig) -> Self
        where D: BatchDatabase {
        let mut v: Vec<Box<dyn Policy + 'a>> = Vec::new();
        v.push(Box::new(ValuePolicy::new(&policy_config, wallet)));
        v.push(Box::new(AndonPolicy::new(&policy_config)));
        return PolicySet {
            policies: v
        };
    }

    pub fn check_policies(&self, psbt: &PartiallySignedTransaction) -> Result<(), Vec<String>> {
        let errors: Vec<String> = self.policies.iter()
            .map(|p| p.check_transaction(psbt))
            .filter(|p| p.is_err())
            .map(|p| p.unwrap_err())
            .collect();
        if errors.is_empty() {
            return Ok(())
        } else {
            return Err(errors)
        }
    }
}

pub async fn get_policy_config_from_ddb(aws_config: &Config, wallet_name: &str) -> Result<PolicyConfig, aws_sdk_dynamodb::Error> {
    let table_name = env::var("POLICY_CONFIG").unwrap();
    let ddb_client = aws_sdk_dynamodb::Client::new(aws_config);
    let gio = ddb_client.get_item()
        .table_name(table_name)
        .key("wallet_name", AttributeValue::S(wallet_name.to_string()))
        .send()
        .await?;
    let item = gio.item().unwrap();


    let config = PolicyConfig {
        wallet_name: wallet_name.to_string(),
        max_spend_per_tx: u64::from_str(item.get("max_spend_per_tx").unwrap_or(&AttributeValue::N("500000".to_string())).as_n().unwrap()).unwrap(),
        all_tx_halted: *item.get("all_tx_halted").unwrap().as_bool().unwrap(),
    };
    info!("Constructed policy config for wallet {} with config: {:?}", config.wallet_name, config);
    Ok(config)
}