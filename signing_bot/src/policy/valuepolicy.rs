use bdk::bitcoin::util::psbt::PartiallySignedTransaction;
use bdk::database::BatchDatabase;
use bdk::Wallet;
use log::{info, warn};

use crate::policy::{Policy, PolicyConfig};

pub(crate) struct ValuePolicy<'a, B, D>
    where D: BatchDatabase {
    wallet: &'a Wallet<B, D>,
    max_spend_per_tx: u64,
}

impl<'a, B, D> ValuePolicy<'a, B, D>
    where D: BatchDatabase {
    pub(crate) fn new(policy_config: &PolicyConfig, wallet: &'a Wallet<B, D>) -> Self {
        let v: ValuePolicy<'a, B, D> = ValuePolicy {
            wallet,
            max_spend_per_tx: policy_config.max_spend_per_tx,
        };
        return v;
    }
}

impl<'a, B, D> Policy for ValuePolicy<'a, B, D>
    where D: BatchDatabase {
    fn check_transaction(&self, psbt: &PartiallySignedTransaction) -> Result<(), String> {
        let tx = psbt.to_owned().extract_tx();
        info!("tx outputs: {:?}", tx.output);
        info!("Filtered outputs: {:?}", tx.output.iter()
            .filter(|txout| !self.wallet.is_mine(&txout.script_pubkey).unwrap()).collect::<Vec<_>>());
        let total_spend = tx.output.iter()
            .filter(|txout| !self.wallet.is_mine(&txout.script_pubkey).unwrap())
            .fold(0, |acc, txout| acc + txout.value);
        info!("Total spend detected: {}", total_spend);
        if self.max_spend_per_tx >= total_spend {
            Ok(())
        } else {
            warn!("{}", format!("Value policy check failed: output total of {} is higher than configured policy limit of {}", total_spend, self.max_spend_per_tx));
            Err(format!("Transaction spend total of {} exceeds policy limit.", total_spend))
        }
    }
}

#[cfg(test)]
mod tests {
    use bdk::FeeRate;
    use bdk::wallet::{AddressIndex, get_funded_wallet};

    use crate::policy::Policy;
    use crate::policy::valuepolicy::ValuePolicy;

    #[test]
    fn value_policy_allows_conformant_tx() {
        let alice_wallet = get_funded_wallet("wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/0/*)").0;
        let bob_wallet = get_funded_wallet("wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/1/*)").0;
        let bob_address = bob_wallet.get_address(AddressIndex::New).unwrap();
        let mut builder = alice_wallet.build_tx();
        builder
            .add_recipient(bob_address.script_pubkey(), 20_000)
            .fee_rate(FeeRate::from_sat_per_vb(5.0));
        let (psbt, _) = builder.finish().unwrap();
        let policy = ValuePolicy {
            wallet: &alice_wallet,
            max_spend_per_tx: 50_000,
        };
        assert!(policy.check_transaction(&psbt).is_ok());
    }

    #[test]
    fn value_policy_doesnt_allow_nonconformant_tx() {
        let alice_wallet = get_funded_wallet("wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/0/*)").0;
        let bob_wallet = get_funded_wallet("wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/1/*)").0;
        let bob_address = bob_wallet.get_address(AddressIndex::New).unwrap();
        let mut builder = alice_wallet.build_tx();
        builder
            .add_recipient(bob_address.script_pubkey(), 20_000)
            .fee_rate(FeeRate::from_sat_per_vb(5.0));
        let (psbt, _) = builder.finish().unwrap();
        let policy = ValuePolicy {
            wallet: &alice_wallet,
            max_spend_per_tx: 10_000,
        };
        assert!(policy.check_transaction(&psbt).is_err());
    }
}