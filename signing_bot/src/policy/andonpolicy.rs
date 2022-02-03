use bdk::bitcoin::util::psbt::PartiallySignedTransaction;
use log::warn;

use crate::policy::{Policy, PolicyConfig};

/// Andon-cord policy. Holds a flag that if set, will refuse to sign any transaction.
/// This will be wired up to some mechanism where you can "hit the big red button" to prevent all
/// spends until you go and reset it.
pub(crate) struct AndonPolicy {
    all_tx_halted: bool,
}

impl AndonPolicy {
    pub(crate) fn new(policy_config: &PolicyConfig) -> Self {
        AndonPolicy {
            all_tx_halted: policy_config.all_tx_halted,
        }
    }

    pub fn halt_all(&mut self) {
        self.all_tx_halted = true;
    }

    pub fn reset(&mut self) {
        self.all_tx_halted = false;
    }
}

impl Policy for AndonPolicy {
    fn check_transaction(&self, _psbt: &PartiallySignedTransaction) -> Result<(), String> {
        if self.all_tx_halted {
            warn!("Transaction signing blocked because andon cord has been pulled");
            return Err(format!("All transactions have been halted"));
        } else {
            return Ok(());
        }
    }
}


#[cfg(test)]
mod tests {
    use bdk::FeeRate;
    use bdk::wallet::{AddressIndex, get_funded_wallet};

    use crate::policy::{Policy, PolicyConfig};
    use crate::policy::andonpolicy::AndonPolicy;

    #[test]
    fn andon_policy_stops_spends() {
        let alice_wallet = get_funded_wallet("wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/0/*)").0;
        let bob_wallet = get_funded_wallet("wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/1/*)").0;
        let bob_address = bob_wallet.get_address(AddressIndex::New).unwrap();
        let mut builder = alice_wallet.build_tx();
        builder
            .add_recipient(bob_address.script_pubkey(), 20_000)
            .fee_rate(FeeRate::from_sat_per_vb(5.0));
        let (psbt, _) = builder.finish().unwrap();
        let config = PolicyConfig {
            wallet_name: "".to_string(),
            max_spend_per_tx: 0,
            all_tx_halted: false,
        };
        let mut policy = AndonPolicy::new(&config);
        assert!(policy.check_transaction(&psbt).is_ok());

        policy.halt_all();
        assert!(policy.check_transaction(&psbt).is_err());

        policy.reset();
        assert!(policy.check_transaction(&psbt).is_ok());
    }
}