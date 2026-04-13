#![allow(dead_code)]

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

const READY_PREFIX: &str = "READY ";

#[derive(Clone)]
pub struct BackendClient {
    base_url: Arc<String>,
}

pub struct BackendProcess {
    client: BackendClient,
    child: Child,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyBody {}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletSummaryRequest {
    pub network: String,
    pub keys_file: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletSummary {
    pub network: String,
    pub keys_file: String,
    pub public_keys: Vec<String>,
    pub sorted_public_keys: Vec<String>,
    pub public_key_count: usize,
    pub owned_key_count: usize,
    pub minimum_signatures: u32,
    pub cosigner_index: u32,
    pub last_used_external_index: u32,
    pub last_used_internal_index: u32,
    pub fingerprint: String,
    pub is_multisig: bool,
    pub has_private_keys: bool,
    pub owns_all_keys: bool,
    pub is_canonical_address_owner: bool,
    pub ecdsa: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapCreateRequest {
    pub network: String,
    pub keys_file: String,
    pub password: String,
    pub minimum_signatures: u32,
    pub num_private_keys: u32,
    pub num_public_keys: u32,
    pub remote_public_keys: Vec<String>,
    pub import_mnemonics: Vec<String>,
    pub ecdsa: bool,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapCreateResponse {
    pub summary: WalletSummary,
    pub local_extended_pub_keys: Vec<String>,
    pub local_mnemonics: Vec<String>,
    pub canonical_owner_warning: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSecretsRequest {
    pub network: String,
    pub keys_file: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSecretsResponse {
    pub mnemonics: Vec<String>,
    pub external_public_keys: Vec<String>,
    pub minimum_signatures: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStartRequest {
    pub network: String,
    pub keys_file: String,
    pub rpc_server: String,
    pub listen: String,
    pub timeout_seconds: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub state: String,
    pub message: String,
    pub daemon_address: String,
    pub network: String,
    pub keys_file: String,
    pub rpc_server: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub wallet_version: Option<String>,
    pub wallet: Option<WalletSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressBalance {
    pub address: String,
    pub available_sompi: u64,
    pub pending_sompi: u64,
    pub available_kas: String,
    pub pending_kas: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceResponse {
    pub available_sompi: u64,
    pub pending_sompi: u64,
    pub available_kas: String,
    pub pending_kas: String,
    pub addresses: Vec<AddressBalance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressesResponse {
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAddressResponse {
    pub address: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeePolicyRequest {
    pub exact_fee_rate: Option<f64>,
    pub max_fee_rate: Option<f64>,
    pub max_fee: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUnsignedRequest {
    pub to_address: String,
    pub amount_kas: String,
    pub send_all: bool,
    pub from_addresses: Vec<String>,
    pub use_existing_change_address: bool,
    pub fee_policy: FeePolicyRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignRequest {
    pub network: String,
    pub keys_file: String,
    pub password: String,
    pub transactions_hex: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastRequest {
    pub transactions_hex: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseRequest {
    pub network: String,
    pub keys_file: String,
    pub transactions_hex: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastResponse {
    pub tx_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionBundle {
    pub transactions_hex: String,
    pub transaction_count: usize,
    pub fully_signed: bool,
    pub transactions: Vec<ParsedTransaction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTransaction {
    pub index: usize,
    pub tx_id: String,
    pub fully_signed: bool,
    pub input_count: usize,
    pub output_count: usize,
    pub inputs: Vec<ParsedInput>,
    pub outputs: Vec<ParsedOutput>,
    pub fee_sompi: u64,
    pub fee_kas: String,
    pub mass: u64,
    pub fee_rate: f64,
    pub has_mass_estimate: bool,
    pub signatures: Vec<InputSignatureProgress>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedInput {
    pub outpoint: String,
    pub amount_sompi: u64,
    pub amount_kas: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedOutput {
    pub address: String,
    pub amount_sompi: u64,
    pub amount_kas: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputSignatureProgress {
    pub input_index: usize,
    pub signed_by: usize,
    pub minimum_signatures: u32,
    pub missing_signatures: u32,
}

impl BackendProcess {
    pub fn spawn() -> Result<Self, String> {
        let backend_bin = std::env::var("KASPAWALLET_GUI_BACKEND_BIN")
            .unwrap_or_else(|_| env!("KASPAWALLET_GUI_BACKEND_BIN").to_owned());

        let mut child = Command::new(backend_bin)
            .arg("--listen")
            .arg("127.0.0.1:0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("failed to launch local Go backend: {err}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "backend stdout pipe is unavailable".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "backend stderr pipe is unavailable".to_owned())?;

        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<String, String>>(1);

        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut ready_sent = false;

            for line_result in reader.lines() {
                match line_result {
                    Ok(line) => {
                        if !ready_sent {
                            if let Some(base_url) = line.strip_prefix(READY_PREFIX) {
                                let _ = ready_tx.send(Ok(base_url.trim().to_owned()));
                                ready_sent = true;
                                continue;
                            }
                        }
                    }
                    Err(err) => {
                        if !ready_sent {
                            let _ =
                                ready_tx.send(Err(format!("failed reading backend stdout: {err}")));
                            ready_sent = true;
                        }
                        break;
                    }
                }
            }

            if !ready_sent {
                let _ =
                    ready_tx.send(Err("backend exited before it reported readiness".to_owned()));
            }
        });

        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for _ in reader.lines() {}
        });

        let base_url = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "timed out waiting for the local Go backend to start".to_owned())??;

        Ok(Self {
            client: BackendClient {
                base_url: Arc::new(base_url),
            },
            child,
        })
    }

    pub fn client(&self) -> BackendClient {
        self.client.clone()
    }
}

impl Drop for BackendProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl BackendClient {
    pub fn wallet_summary(&self, request: &WalletSummaryRequest) -> Result<WalletSummary, String> {
        self.post("/api/wallet/summary", request)
    }

    pub fn create_wallet(
        &self,
        request: &BootstrapCreateRequest,
    ) -> Result<BootstrapCreateResponse, String> {
        self.post("/api/bootstrap/create", request)
    }

    pub fn export_secrets(
        &self,
        request: &ExportSecretsRequest,
    ) -> Result<ExportSecretsResponse, String> {
        self.post("/api/bootstrap/export-secrets", request)
    }

    pub fn start_daemon(&self, request: &DaemonStartRequest) -> Result<DaemonStatus, String> {
        self.post("/api/daemon/start", request)
    }

    pub fn stop_daemon(&self) -> Result<DaemonStatus, String> {
        self.post("/api/daemon/stop", &EmptyBody {})
    }

    pub fn daemon_status(&self) -> Result<DaemonStatus, String> {
        self.post("/api/daemon/status", &EmptyBody {})
    }

    pub fn balance(&self) -> Result<BalanceResponse, String> {
        self.post("/api/balance", &EmptyBody {})
    }

    pub fn list_addresses(&self) -> Result<AddressesResponse, String> {
        self.post("/api/addresses/list", &EmptyBody {})
    }

    pub fn new_address(&self) -> Result<NewAddressResponse, String> {
        self.post("/api/addresses/new", &EmptyBody {})
    }

    pub fn create_unsigned(
        &self,
        request: &CreateUnsignedRequest,
    ) -> Result<TransactionBundle, String> {
        self.post("/api/transactions/create-unsigned", request)
    }

    pub fn sign(&self, request: &SignRequest) -> Result<TransactionBundle, String> {
        self.post("/api/transactions/sign", request)
    }

    pub fn broadcast(&self, request: &BroadcastRequest) -> Result<BroadcastResponse, String> {
        self.post("/api/transactions/broadcast", request)
    }

    pub fn parse(&self, request: &ParseRequest) -> Result<TransactionBundle, String> {
        self.post("/api/transactions/parse", request)
    }

    fn post<TRequest, TResponse>(&self, path: &str, request: &TRequest) -> Result<TResponse, String>
    where
        TRequest: Serialize,
        TResponse: DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, path);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(120))
            .timeout_write(Duration::from_secs(30))
            .build();

        match agent.post(&url).send_json(request) {
            Ok(response) => {
                let body = response
                    .into_string()
                    .map_err(|err| format!("failed reading bridge response: {err}"))?;
                serde_json::from_str::<TResponse>(&body)
                    .map_err(|err| format!("failed decoding bridge response from {path}: {err}"))
            }
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                if let Ok(api_error) = serde_json::from_str::<ApiError>(&body) {
                    Err(api_error.error)
                } else if body.trim().is_empty() {
                    Err(format!("bridge request failed with HTTP {status}"))
                } else {
                    Err(format!("bridge request failed with HTTP {status}: {body}"))
                }
            }
            Err(ureq::Error::Transport(err)) => Err(format!("bridge transport error: {err}")),
        }
    }
}
