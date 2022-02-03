use std::env;
use std::str::FromStr;

use aws_config::Config;
use bdk::{SignOptions, Wallet};
use bdk::bitcoin::{base64, Network};
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::util::bip32::{DerivationPath, KeySource};
use bdk::bitcoin::util::psbt::PartiallySignedTransaction as Psbt;
use bdk::database::MemoryDatabase;
use bdk::descriptor::Segwitv0;
use bdk::keys::{DerivableKey, DescriptorKey, ExtendedKey};
use bdk::keys::bip39::Mnemonic;
use bdk::keys::DescriptorKey::Secret;
use bdk::wallet::{AddressIndex, wallet_name_from_descriptor};
use bdk::wallet::export::WalletExport;
use lambda_http::{Body, handler, Request, RequestExt, Response, StrMap};
use lambda_http::Body::Text;
use lambda_http::http::Method;
use lambda_runtime::Context;
use log::{debug, error, info};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::policy::{get_policy_config_from_ddb, PolicySet};

mod policy;

#[derive(Debug, Deserialize, Default)]
struct NewKeyRequest {
    key_name: String,
}

#[derive(Debug, Serialize)]
struct FailureResponse {
    pub body: String,
}

#[derive(Deserialize, Serialize)]
struct SavedKey {
    fingerprint: String,
    mnemonic: String,
    xprv: String,
    xpub: String,
}

// Implement Display for the Failure response so that we can then implement Error.
impl std::fmt::Display for FailureResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.body)
    }
}

// Implement Error for the FailureResponse so that we can `?` (try) the Response
// returned by `lambda_runtime::run(func).await` in `fn main`.
impl std::error::Error for FailureResponse {}

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;


#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // go look at logs in CWL
    tracing_subscriber::fmt::init();
    debug!("logger has been set up");

    lambda_runtime::run(handler(handle_req)).await?;
    Ok(())
}

async fn handle_req(req: Request, _ctx: Context) -> Result<Response<Body>, Error> {
    println!("Got request with path parameters: {:?}", req.path_parameters());
    println!("Got request with uri {:?}", req.uri());
    println!("Got request with path: {:?}", req.uri().path());
    println!("Request body: {:?}", req.body());

    let bucket = env::var("BUCKET").unwrap_or("seedbucket-1234567890".to_string());
    let derivation_path = env::var("DERIVATION_PATH").unwrap_or("m/48'/0'/0'/2'".to_string());
    let config = aws_config::load_from_env().await;

    /*
    API:
    POST key_name to /keys -> create new key
    GET /keys/{key} -> get xpub
    POST bluewallet_export to /keys/{key} -> create wallet
    POST psbt to /keys/{key}/wallet -> sign psbt
     */

    let path = req.uri().path().strip_suffix("/").unwrap_or(req.uri().path());
    let path_params = req.path_parameters();
    if req.method() == Method::POST && path_params.is_empty() && path == "/keys" {
        // Create Key API
        return create_new_keypair(&req, &bucket, &derivation_path, &config).await;
    } else if req.method() == Method::GET && path_params.get("key").is_some() && !path.ends_with("/wallet") {
        // Get xpub API
        return Ok(Response::new(format!("Not Implemented: Get the xpub").into()));
    } else if req.method() == Method::POST && path_params.get("key").is_some() && !path.ends_with("/wallet") {
        // Create wallet API
        return create_new_wallet(&req, &bucket, &config, &path_params).await;
    } else if req.method() == Method::POST && path_params.get("key").is_some() && path.ends_with("/wallet") {
        // Sign PSBT API
        return sign_psbt(req, &bucket, &config, path_params).await;
    } else {
        // Unhandled path/params. This should never get called because API-GW *should* catch it first
        return Err(Box::new(FailureResponse {
            body: "Not supported operation".to_string()
        }));
    }
}

async fn sign_psbt(req: Request, bucket: &String, config: &Config, path_params: StrMap) -> Result<Response<Body>, Error> {
    let key_name = path_params.get("key").unwrap().to_string();
    let b64_psbt;
    if let Text(psbt) = req.body() {
        b64_psbt = psbt.to_string();
    } else {
        return Err(Box::new(FailureResponse { body: "Could read uploaded file".to_string() }));
    }
    let s3_client = aws_sdk_s3::Client::new(&config);
    let fetched_object = s3_client.get_object()
        .bucket(bucket)
        .key(format!("{}-wallet", &key_name))
        .send()
        .await
        .map_err(|err| {
            error!("Could not fetch wallet from s3: {}", err);
            FailureResponse {
                body: "Could not get wallet from storage".to_string()
            }
        })?;
    let import = WalletExport::from_str(std::str::from_utf8(&fetched_object.body.collect().await.unwrap().into_bytes()).unwrap())
        .map_err(|err| {
            error!("Could not parse wallet stored wallet export: {}", err);
            FailureResponse {
                body: "Could not read saved wallet".to_string()
            }
        })?;
    let wallet = Wallet::new_offline(
        &import.descriptor(),
        import.change_descriptor().as_ref(),
        Network::Bitcoin,
        MemoryDatabase::new(),
    ).unwrap();
    let _cached = wallet.ensure_addresses_cached(Some(100));
    let mut psbt = Psbt::from_str(&b64_psbt).map_err(|err| {
        error!("Could not decode psbt: {}", err);
        FailureResponse {
            body: "Could not decode psbt".to_string()
        }
    })?;

    let secp = Secp256k1::new();
    // let wallet_name = wallet_name_from_descriptor(&import.descriptor(), *&import.change_descriptor().as_ref(), Network::Bitcoin, &secp).unwrap();
    let policy_config = get_policy_config_from_ddb(&config, &key_name).await?;
    let policies = PolicySet::new(&wallet, &policy_config);
    return match policies.check_policies(&psbt) {
        Ok(_) => {
            info!("Passed value policy check");
            let _signed = wallet.sign(&mut psbt, SignOptions::default()).unwrap();
            Ok(Response::new(psbt.to_string().into()))
        }
        Err(_) => {
            Err(Box::new(FailureResponse {
                body: "Transaction failed policy checks".to_string()
            }))
        }
    };
}

async fn create_new_wallet(req: &Request, bucket: &String, config: &Config, path_params: &StrMap) -> Result<Response<Body>, Error> {
    let key_name = path_params.get("key").unwrap().to_string();
    let setup_file_b64;
    if let Text(setup_file) = req.body() {
        setup_file_b64 = setup_file.to_string();
    } else {
        return Err(Box::new(FailureResponse { body: "Could read uploaded file".to_string() }));
    }
    let mut dc = DescriptorComponents::from_bluewallet_export(std::str::from_utf8(&base64::decode(&setup_file_b64).unwrap()).unwrap());
    debug!("Got descriptor from upload: {}", dc.into_main_descriptor());
    let s3_client = aws_sdk_s3::Client::new(&config);
    let fetched_object = s3_client.get_object()
        .bucket(bucket)
        .key(&key_name)
        .send()
        .await
        .map_err(|err| {
            error!("Could not fetch seed from s3: {}", err);
            FailureResponse {
                body: "Could not get seed from storage".to_string()
            }
        })?;
    let saved_key: SavedKey = serde_json::from_str(std::str::from_utf8(&fetched_object.body.collect().await.unwrap().into_bytes()).unwrap()).unwrap();
    let new_keys = dc.keys.iter().map(|key| {
        let fprint = key.0.to_lowercase();
        if fprint == saved_key.fingerprint {
            let k = saved_key.xprv.clone();
            // get rid of the fingerprint/path and the training path
            let trimmed: String = k.split("]").last().unwrap().split("/").nth(0).unwrap().to_string();
            (fprint, trimmed)
        } else {
            (fprint, key.clone().1)
        }
    }).collect();
    dc.keys = new_keys;

    let descriptor = dc.into_main_descriptor();
    let change = dc.into_change_descriptor();
    println!("Assembled descriptor: {}", descriptor);
    let wallet = Wallet::new_offline(
        &descriptor,
        Some(&change),
        Network::Bitcoin,
        MemoryDatabase::new(),
    ).unwrap();
    debug!("first address from the new wallet: {}", wallet.get_address(AddressIndex::New).unwrap().address.to_string());

    let export = WalletExport::export_wallet(&wallet, &key_name, false)
        .map_err(|err| {
            error!("Could not export wallet: {}", err);
            FailureResponse {
                body: "Could not export wallet".to_string()
            }
        })?;
    s3_client.put_object()
        .bucket(bucket)
        .key(format!("{}-wallet", key_name))
        .body(export.to_string().as_bytes().to_owned().into())
        .send()
        .await
        .map_err(|err| {
            error!("Could not save wallet: {}", err);
            FailureResponse {
                body: "Could not save wallet".to_string()
            }
        })?;
    return Ok(Response::new(format!("First address from the wallet: {}", wallet.get_address(AddressIndex::New).unwrap().to_string()).into()));
}

async fn create_new_keypair(req: &Request, bucket: &String, derivation_path: &String, config: &Config) -> Result<Response<Body>, Error> {
    let args: NewKeyRequest = req.payload().unwrap().unwrap();
    let key_name = args.key_name;
    let kms_client = aws_sdk_kms::Client::new(&config);
    let random_str = kms_client.generate_random()
        .number_of_bytes(32)
        .send()
        .await
        .map_err(|err| {
            // in case of failure, log it to CWL
            error!("Failed to get random bytes from KMS with error: {}", err);
            FailureResponse {
                body: "Could not get entropy to generate seed".to_string()
            }
        })?;
    let mnemonic = Mnemonic::from_entropy(random_str.plaintext.unwrap().as_ref()).unwrap();
    let mnemonic_string = mnemonic.to_string();

    let secp = Secp256k1::new();
    let xkey: ExtendedKey = mnemonic.into_extended_key().unwrap();
    let xprv = xkey.into_xprv(Network::Bitcoin).unwrap();
    let fingerprint = xprv.fingerprint(&secp);
    let derived_xprv = &xprv.derive_priv(&secp, &DerivationPath::from_str(&derivation_path).unwrap()).unwrap();
    let origin: KeySource = (xprv.fingerprint(&secp), derivation_path.parse().unwrap());
    let derived_xprv_desc_key: DescriptorKey<Segwitv0> = derived_xprv.into_descriptor_key(Some(origin), DerivationPath::default()).unwrap();

    if let Secret(desc_seckey, _, _) = derived_xprv_desc_key {
        let xpub = desc_seckey.as_public(&secp).unwrap();
        info!("The master fingerprint is {}", desc_seckey.as_public(&secp).unwrap().master_fingerprint());
        let saved_key = SavedKey {
            fingerprint: fingerprint.to_string(),
            mnemonic: mnemonic_string,
            xprv: desc_seckey.to_string(),
            xpub: xpub.to_string(),
        };
        let s3_client = aws_sdk_s3::Client::new(&config);
        s3_client.put_object()
            .bucket(bucket)
            .key(key_name)
            .body(serde_json::to_string(&saved_key).unwrap().as_bytes().to_owned().into())
            .send()
            .await
            .map_err(|err| {
                error!("Failed to put seed in s3: {}", err);
                FailureResponse {
                    body: "Could not put mnemonic in S3 bucket".to_string()
                }
            })?;
        return Ok(Response::new(xpub.to_string().into()));
    } else {
        return Err(Box::new(FailureResponse {
            body: "Could not format xpub".to_string()
        }));
    }
}


struct DescriptorComponents {
    derivation: Option<String>,
    threshold: Option<String>,
    format: Option<String>,
    keys: Vec<(String, String)>,
}

impl DescriptorComponents {
    fn into_change_descriptor(&self) -> String {
        self.into_descriptor_str("1")
    }

    fn into_main_descriptor(&self) -> String {
        self.into_descriptor_str("0")
    }

    fn into_descriptor_str(&self, desc_type: &str) -> String {
        let mut descriptor = "wsh(sortedmulti(".to_string(); // TODO: handle other script types
        descriptor.push_str(&self.threshold.as_ref().unwrap());
        descriptor.push_str(self.keys.iter().fold("".to_string(), |acc, key| {
            acc + format!(",[{}/{}]{}/{}/*", key.0, self.derivation.as_ref().unwrap().strip_prefix("m/").unwrap(), key.1, desc_type).as_str()
        }).as_str());
        descriptor.push_str("))");
        return descriptor;
    }

    fn from_bluewallet_export(bw_export: &str) -> Self {
        let mut components = DescriptorComponents {
            derivation: None,
            threshold: None,
            format: None,
            keys: Vec::new(),
        };
        let comment_regex = Regex::new("^#").unwrap();
        // TODO: figure out if I want to handle the name later
        // let name_regex = Regex::new(r"^Name: (.+)").unwrap();
        let policy_regex = Regex::new(r"^Policy: (\d) of \d").unwrap();
        let derivation_regex = Regex::new(r"^Derivation: (.+)").unwrap();
        // we're not actually going to use the format until we support other script types
        let format_regex = Regex::new(r"^Format: (.+)").unwrap();
        let key_regex = Regex::new(r"^([A-F0-9]{8}): (.+)").unwrap();
        bw_export.lines()
            .filter(|line| !comment_regex.is_match(line))
            .for_each(|line| {
                if let Some(caps) = policy_regex.captures(line) {
                    components.threshold = Some(caps.get(1).unwrap().as_str().to_string());
                }
                if let Some(caps) = derivation_regex.captures(line) {
                    components.derivation = Some(caps.get(1).unwrap().as_str().to_string());
                }
                if let Some(caps) = format_regex.captures(line) {
                    components.format = Some(caps.get(1).unwrap().as_str().to_string());
                }
                if let Some(caps) = key_regex.captures(line) {
                    components.keys.push((caps.get(1).unwrap().as_str().to_string(), caps.get(2).unwrap().as_str().to_string()));
                }
            });
        return components;
    }
}

#[cfg(test)]
mod tests {
    use crate::DescriptorComponents;

    #[test]
    fn can_parse_bluewallet_file() {
        let bluewallet_file_test = "# BlueWallet Multisig setup file
# this file contains only public keys and is safe to
# distribute among cosigners
#
Name: test5678
Policy: 2 of 3
Derivation: m/48'/0'/0'/2'
Format: P2WSH

EAB239AA: xpub6E2HG1bNB69EfRnM8vX2vCktifqLHnQH9Har7ZwWegwkss43rEa5EkJnCjiUKMnV5DRKQJUMCaiysNTq12RZ6cffhJbJtXp4atScMDF83SC

F843467D: xpub6EzLSnj1J7ZVK2o4HuU9pwyDfY6uF1wTpSH2g2dZy13oxqyXEJRb44PbeRrcXDaVLFhHq3MVxuzEfiRZBuCcETuNY7z2rNrNudBY7gZrWYu

16EFEC75: xpub6EFHgaRm1rd3AE8DmxXVncR4RcBsirn4ncDc2mW1oCThkQosh7Rdu6SdyugwWBZV97usQf5WwUn89UaH7bVRoZ5NY8sdwpt8H7Zi9ayhLk5

";
        let descriptor = DescriptorComponents::from_bluewallet_export(bluewallet_file_test).into_main_descriptor();
        assert_eq!(descriptor, "wsh(sortedmulti(2,[EAB239AA/48'/0'/0'/2']xpub6E2HG1bNB69EfRnM8vX2vCktifqLHnQH9Har7ZwWegwkss43rEa5EkJnCjiUKMnV5DRKQJUMCaiysNTq12RZ6cffhJbJtXp4atScMDF83SC/0/*,[F843467D/48'/0'/0'/2']xpub6EzLSnj1J7ZVK2o4HuU9pwyDfY6uF1wTpSH2g2dZy13oxqyXEJRb44PbeRrcXDaVLFhHq3MVxuzEfiRZBuCcETuNY7z2rNrNudBY7gZrWYu/0/*,[16EFEC75/48'/0'/0'/2']xpub6EFHgaRm1rd3AE8DmxXVncR4RcBsirn4ncDc2mW1oCThkQosh7Rdu6SdyugwWBZV97usQf5WwUn89UaH7bVRoZ5NY8sdwpt8H7Zi9ayhLk5/0/*))");
    }
}