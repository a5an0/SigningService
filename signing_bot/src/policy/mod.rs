use bdk::{KeychainKind, Wallet};
use bdk::bitcoin::Network;
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::util::psbt::PartiallySignedTransaction;
use bdk::database::BatchDatabase;
use bdk::wallet::wallet_name_from_descriptor;

use crate::policy::andonpolicy::AndonPolicy;
use crate::policy::valuepolicy::ValuePolicy;

mod andonpolicy;
mod valuepolicy;

pub(crate) struct PolicyConfig {
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
    pub fn new<B, D>(wallet: &'a Wallet<B, D>) -> Self
        where D: BatchDatabase {
        // TODO: read this out of DDB
        let policy_config = PolicyConfig {
            wallet_name: wallet_name_from_descriptor(wallet.get_descriptor_for_keychain(KeychainKind::External).to_owned(),
                                                     Option::from(wallet.get_descriptor_for_keychain(KeychainKind::Internal).to_owned()),
                                                     Network::Bitcoin, &Secp256k1::gen_new()).unwrap(),
            max_spend_per_tx: 500_000,
            all_tx_halted: false,
        };
        println!("constructed config for wallet named: {}", &policy_config.wallet_name);
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
