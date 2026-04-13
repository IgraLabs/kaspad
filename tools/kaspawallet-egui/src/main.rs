mod backend;
mod theme;

use backend::{
    AddressesResponse, BackendClient, BackendProcess, BalanceResponse, BootstrapCreateRequest,
    BootstrapCreateResponse, BroadcastRequest, BroadcastResponse, CreateUnsignedRequest,
    DaemonStartRequest, DaemonStatus, ExportSecretsRequest, ExportSecretsResponse,
    FeePolicyRequest, NewAddressResponse, ParseRequest, SignRequest, TransactionBundle,
    WalletSummary, WalletSummaryRequest,
};
use eframe::egui::{self, Align, Color32, ComboBox, Frame, Layout, RichText, Stroke, TextEdit, Ui};
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

const INK: Color32 = Color32::from_rgb(45, 37, 33);
const COPPER: Color32 = Color32::from_rgb(194, 102, 43);
const TEAL: Color32 = Color32::from_rgb(0, 109, 118);
const SAND: Color32 = Color32::from_rgb(248, 242, 233);
const CREAM: Color32 = Color32::from_rgb(255, 252, 248);
const WARM_RED: Color32 = Color32::from_rgb(176, 72, 61);
const OLIVE: Color32 = Color32::from_rgb(95, 116, 62);

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1480.0, 980.0])
            .with_min_inner_size([1200.0, 780.0])
            .with_title("Kaspa Multisig Control Room"),
        ..Default::default()
    };

    eframe::run_native(
        "Kaspa Multisig Control Room",
        native_options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(WalletApp::new()))
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NetworkChoice {
    Mainnet,
    Testnet,
    Devnet,
    Simnet,
}

impl NetworkChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Testnet => "testnet",
            Self::Devnet => "devnet",
            Self::Simnet => "simnet",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Mainnet => "Mainnet",
            Self::Testnet => "Testnet",
            Self::Devnet => "Devnet",
            Self::Simnet => "Simnet",
        }
    }

    fn default_rpc_server(self) -> &'static str {
        match self {
            Self::Mainnet => "stage-roman.igralabs.com:16210",
            Self::Testnet | Self::Devnet | Self::Simnet => "localhost",
        }
    }

    fn default_keys_file(self) -> String {
        format!("~/.kaspawallet/{}/igra_msig_keys.json", self.as_str())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FeeMode {
    Estimate,
    ExactFeeRate,
    MaxFeeRate,
    MaxFee,
}

impl FeeMode {
    fn label(self) -> &'static str {
        match self {
            Self::Estimate => "Node estimate",
            Self::ExactFeeRate => "Exact fee rate",
            Self::MaxFeeRate => "Cap fee rate",
            Self::MaxFee => "Cap total fee",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageTone {
    Info,
    Error,
}

struct BannerMessage {
    tone: MessageTone,
    text: String,
}

struct BootstrapForm {
    network: NetworkChoice,
    keys_file: String,
    password: String,
    minimum_signatures: u32,
    num_private_keys: u32,
    num_public_keys: u32,
    remote_public_keys: String,
    import_mode: bool,
    import_mnemonics: String,
    overwrite: bool,
    ecdsa: bool,
}

impl BootstrapForm {
    fn new() -> Self {
        let network = NetworkChoice::Mainnet;
        Self {
            network,
            keys_file: network.default_keys_file(),
            password: String::new(),
            minimum_signatures: 2,
            num_private_keys: 1,
            num_public_keys: 3,
            remote_public_keys: String::new(),
            import_mode: false,
            import_mnemonics: String::new(),
            overwrite: false,
            ecdsa: false,
        }
    }

    fn remote_public_keys_vec(&self) -> Vec<String> {
        split_lines(&self.remote_public_keys)
    }

    fn import_mnemonics_vec(&self) -> Vec<String> {
        self.import_mnemonics
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn sync_defaults(&mut self) {
        self.keys_file = self.network.default_keys_file();
    }
}

struct DaemonForm {
    rpc_server: String,
    listen: String,
}

impl DaemonForm {
    fn new(network: NetworkChoice) -> Self {
        Self {
            rpc_server: network.default_rpc_server().to_owned(),
            listen: String::new(),
        }
    }

    fn sync_defaults(&mut self, network: NetworkChoice) {
        self.rpc_server = network.default_rpc_server().to_owned();
        self.listen.clear();
    }
}

struct SpendForm {
    to_address: String,
    amount_kas: String,
    send_all: bool,
    from_addresses: String,
    use_existing_change_address: bool,
    fee_mode: FeeMode,
    fee_value: String,
}

impl SpendForm {
    fn new() -> Self {
        Self {
            to_address: String::new(),
            amount_kas: "1".to_owned(),
            send_all: false,
            from_addresses: String::new(),
            use_existing_change_address: false,
            fee_mode: FeeMode::Estimate,
            fee_value: String::new(),
        }
    }

    fn from_addresses_vec(&self) -> Vec<String> {
        split_lines(&self.from_addresses)
    }

    fn fee_policy(&self) -> Result<FeePolicyRequest, String> {
        let mut policy = FeePolicyRequest::default();
        match self.fee_mode {
            FeeMode::Estimate => {}
            FeeMode::ExactFeeRate => {
                policy.exact_fee_rate = Some(parse_f64_field("fee rate", &self.fee_value)?);
            }
            FeeMode::MaxFeeRate => {
                policy.max_fee_rate = Some(parse_f64_field("max fee rate", &self.fee_value)?);
            }
            FeeMode::MaxFee => {
                policy.max_fee = Some(parse_u64_field("max fee", &self.fee_value)?);
            }
        }
        Ok(policy)
    }
}

struct InspectorForm {
    transactions_hex: String,
}

impl InspectorForm {
    fn new() -> Self {
        Self {
            transactions_hex: String::new(),
        }
    }
}

enum AppEvent {
    WalletSummary(Result<WalletSummary, String>),
    WalletCreated(Result<BootstrapCreateResponse, String>),
    SecretsExported(Result<ExportSecretsResponse, String>),
    DaemonStatus(Result<DaemonStatus, String>),
    Balance(Result<BalanceResponse, String>),
    Addresses(Result<AddressesResponse, String>),
    NewAddress(Result<NewAddressResponse, String>),
    FlowBundle(Result<TransactionBundle, String>),
    InspectorBundle(Result<TransactionBundle, String>),
    Broadcasted(Result<BroadcastResponse, String>),
}

struct WalletApp {
    _backend: Option<BackendProcess>,
    bridge: Option<BackendClient>,
    bootstrap: BootstrapForm,
    daemon_form: DaemonForm,
    spend: SpendForm,
    inspector: InspectorForm,
    summary: Option<WalletSummary>,
    secrets: Option<ExportSecretsResponse>,
    daemon_status: Option<DaemonStatus>,
    balance: Option<BalanceResponse>,
    addresses: Vec<String>,
    last_new_address: Option<String>,
    flow_hex: String,
    flow_bundle: Option<TransactionBundle>,
    inspector_bundle: Option<TransactionBundle>,
    last_broadcast_tx_ids: Vec<String>,
    pending: BTreeSet<&'static str>,
    banner: Option<BannerMessage>,
    events_tx: Sender<AppEvent>,
    events_rx: Receiver<AppEvent>,
    last_status_poll: Instant,
    last_balance_poll: Instant,
}

impl WalletApp {
    fn new() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let bootstrap = BootstrapForm::new();
        let daemon_form = DaemonForm::new(bootstrap.network);

        let (backend, bridge, banner) = match BackendProcess::spawn() {
            Ok(process) => {
                let bridge = process.client();
                (
                    Some(process),
                    Some(bridge),
                    Some(BannerMessage {
                        tone: MessageTone::Info,
                        text: "Local Go wallet bridge is running. Load or create a multisig wallet to begin.".to_owned(),
                    }),
                )
            }
            Err(err) => (
                None,
                None,
                Some(BannerMessage {
                    tone: MessageTone::Error,
                    text: err,
                }),
            ),
        };

        Self {
            _backend: backend,
            bridge,
            bootstrap,
            daemon_form,
            spend: SpendForm::new(),
            inspector: InspectorForm::new(),
            summary: None,
            secrets: None,
            daemon_status: None,
            balance: None,
            addresses: Vec::new(),
            last_new_address: None,
            flow_hex: String::new(),
            flow_bundle: None,
            inspector_bundle: None,
            last_broadcast_tx_ids: Vec::new(),
            pending: BTreeSet::new(),
            banner,
            events_tx,
            events_rx,
            last_status_poll: Instant::now() - Duration::from_secs(60),
            last_balance_poll: Instant::now() - Duration::from_secs(60),
        }
    }

    fn network_string(&self) -> String {
        self.bootstrap.network.as_str().to_owned()
    }

    fn spawn_task<F>(&mut self, tag: &'static str, task: F)
    where
        F: FnOnce(BackendClient) -> AppEvent + Send + 'static,
    {
        let Some(bridge) = self.bridge.clone() else {
            self.set_banner(MessageTone::Error, "Local Go bridge is unavailable.");
            return;
        };

        self.pending.insert(tag);
        let sender = self.events_tx.clone();
        thread::spawn(move || {
            let _ = sender.send(task(bridge));
        });
    }

    fn set_banner(&mut self, tone: MessageTone, text: impl Into<String>) {
        self.banner = Some(BannerMessage {
            tone,
            text: text.into(),
        });
    }

    fn request_wallet_summary(&mut self) {
        let request = WalletSummaryRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
        };
        self.spawn_task("summary", move |bridge| {
            AppEvent::WalletSummary(bridge.wallet_summary(&request))
        });
    }

    fn request_wallet_create(&mut self) {
        let request = BootstrapCreateRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
            password: self.bootstrap.password.clone(),
            minimum_signatures: self.bootstrap.minimum_signatures,
            num_private_keys: self.bootstrap.num_private_keys,
            num_public_keys: self.bootstrap.num_public_keys,
            remote_public_keys: self.bootstrap.remote_public_keys_vec(),
            import_mnemonics: if self.bootstrap.import_mode {
                self.bootstrap.import_mnemonics_vec()
            } else {
                Vec::new()
            },
            ecdsa: self.bootstrap.ecdsa,
            overwrite: self.bootstrap.overwrite,
        };
        self.spawn_task("create_wallet", move |bridge| {
            AppEvent::WalletCreated(bridge.create_wallet(&request))
        });
    }

    fn request_export_secrets(&mut self) {
        let request = ExportSecretsRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
            password: self.bootstrap.password.clone(),
        };
        self.spawn_task("export_secrets", move |bridge| {
            AppEvent::SecretsExported(bridge.export_secrets(&request))
        });
    }

    fn request_start_daemon(&mut self) {
        let request = DaemonStartRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
            rpc_server: self.daemon_form.rpc_server.clone(),
            listen: self.daemon_form.listen.clone(),
            timeout_seconds: 30,
        };
        self.spawn_task("start_daemon", move |bridge| {
            AppEvent::DaemonStatus(bridge.start_daemon(&request))
        });
    }

    fn request_stop_daemon(&mut self) {
        self.spawn_task("stop_daemon", move |bridge| {
            AppEvent::DaemonStatus(bridge.stop_daemon())
        });
    }

    fn request_daemon_status(&mut self) {
        self.spawn_task("daemon_status", move |bridge| {
            AppEvent::DaemonStatus(bridge.daemon_status())
        });
    }

    fn request_balance(&mut self) {
        self.spawn_task("balance", move |bridge| AppEvent::Balance(bridge.balance()));
    }

    fn request_addresses(&mut self) {
        self.spawn_task("addresses", move |bridge| {
            AppEvent::Addresses(bridge.list_addresses())
        });
    }

    fn request_new_address(&mut self) {
        self.spawn_task("new_address", move |bridge| {
            AppEvent::NewAddress(bridge.new_address())
        });
    }

    fn request_create_unsigned(&mut self) {
        let fee_policy = match self.spend.fee_policy() {
            Ok(policy) => policy,
            Err(err) => {
                self.set_banner(MessageTone::Error, err);
                return;
            }
        };

        let request = CreateUnsignedRequest {
            to_address: self.spend.to_address.clone(),
            amount_kas: self.spend.amount_kas.clone(),
            send_all: self.spend.send_all,
            from_addresses: self.spend.from_addresses_vec(),
            use_existing_change_address: self.spend.use_existing_change_address,
            fee_policy,
        };
        self.spawn_task("flow_bundle", move |bridge| {
            AppEvent::FlowBundle(bridge.create_unsigned(&request))
        });
    }

    fn request_sign_flow(&mut self) {
        let request = SignRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
            password: self.bootstrap.password.clone(),
            transactions_hex: self.flow_hex.clone(),
        };
        self.spawn_task("flow_bundle", move |bridge| {
            AppEvent::FlowBundle(bridge.sign(&request))
        });
    }

    fn request_parse_flow(&mut self) {
        let request = ParseRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
            transactions_hex: self.flow_hex.clone(),
        };
        self.spawn_task("flow_bundle", move |bridge| {
            AppEvent::FlowBundle(bridge.parse(&request))
        });
    }

    fn request_broadcast_flow(&mut self) {
        let request = BroadcastRequest {
            transactions_hex: self.flow_hex.clone(),
        };
        self.spawn_task("broadcast", move |bridge| {
            AppEvent::Broadcasted(bridge.broadcast(&request))
        });
    }

    fn request_parse_inspector(&mut self) {
        let request = ParseRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
            transactions_hex: self.inspector.transactions_hex.clone(),
        };
        self.spawn_task("inspector", move |bridge| {
            AppEvent::InspectorBundle(bridge.parse(&request))
        });
    }

    fn handle_events(&mut self) {
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                AppEvent::WalletSummary(result) => {
                    self.pending.remove("summary");
                    match result {
                        Ok(summary) => {
                            self.summary = Some(summary);
                            self.set_banner(
                                MessageTone::Info,
                                "Loaded wallet summary from the Go backend.",
                            );
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::WalletCreated(result) => {
                    self.pending.remove("create_wallet");
                    match result {
                        Ok(created) => {
                            self.summary = Some(created.summary.clone());
                            self.secrets = Some(ExportSecretsResponse {
                                mnemonics: created.local_mnemonics.clone(),
                                external_public_keys: created.local_extended_pub_keys.clone(),
                                minimum_signatures: created.summary.minimum_signatures,
                            });
                            self.bootstrap.keys_file = created.summary.keys_file.clone();
                            let message = if created.canonical_owner_warning.is_empty() {
                                "Wallet file created and fingerprint locked in."
                            } else {
                                created.canonical_owner_warning.as_str()
                            };
                            self.set_banner(MessageTone::Info, message);
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::SecretsExported(result) => {
                    self.pending.remove("export_secrets");
                    match result {
                        Ok(secrets) => {
                            self.secrets = Some(secrets);
                            self.set_banner(
                                MessageTone::Info,
                                "Decrypted wallet secrets are on screen. Clear them when finished.",
                            );
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::DaemonStatus(result) => {
                    self.pending.remove("start_daemon");
                    self.pending.remove("stop_daemon");
                    self.pending.remove("daemon_status");
                    match result {
                        Ok(status) => {
                            self.daemon_status = Some(status.clone());
                            if let Some(wallet) = status.wallet.clone() {
                                self.summary = Some(wallet);
                            }
                            self.set_banner(MessageTone::Info, status.message.clone());
                            if status.state == "ready" && !self.pending.contains("balance") {
                                self.request_balance();
                            }
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::Balance(result) => {
                    self.pending.remove("balance");
                    match result {
                        Ok(balance) => {
                            self.balance = Some(balance);
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::Addresses(result) => {
                    self.pending.remove("addresses");
                    match result {
                        Ok(addresses) => {
                            self.addresses = addresses.addresses;
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::NewAddress(result) => {
                    self.pending.remove("new_address");
                    match result {
                        Ok(address) => {
                            self.last_new_address = Some(address.address.clone());
                            if !self.addresses.iter().any(|item| item == &address.address) {
                                self.addresses.insert(0, address.address.clone());
                            }
                            self.set_banner(
                                MessageTone::Info,
                                "Generated the next canonical receive address.",
                            );
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::FlowBundle(result) => {
                    self.pending.remove("flow_bundle");
                    match result {
                        Ok(bundle) => {
                            self.flow_hex = bundle.transactions_hex.clone();
                            self.flow_bundle = Some(bundle);
                            self.set_banner(
                                MessageTone::Info,
                                "Updated the spend pipeline state using the Go wallet backend.",
                            );
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::InspectorBundle(result) => {
                    self.pending.remove("inspector");
                    match result {
                        Ok(bundle) => {
                            self.inspector_bundle = Some(bundle);
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::Broadcasted(result) => {
                    self.pending.remove("broadcast");
                    match result {
                        Ok(response) => {
                            self.last_broadcast_tx_ids = response.tx_ids.clone();
                            self.set_banner(
                                MessageTone::Info,
                                format!("Broadcast {} transaction(s).", response.tx_ids.len()),
                            );
                            if !self.pending.contains("daemon_status") {
                                self.request_daemon_status();
                            }
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
            }
        }
    }

    fn auto_poll(&mut self) {
        let now = Instant::now();
        let daemon_ready = self
            .daemon_status
            .as_ref()
            .map(|status| {
                matches!(
                    status.state.as_str(),
                    "starting" | "syncing" | "running" | "ready"
                )
            })
            .unwrap_or(false);

        if daemon_ready
            && !self.pending.contains("daemon_status")
            && now.duration_since(self.last_status_poll) > Duration::from_secs(3)
        {
            self.last_status_poll = now;
            self.request_daemon_status();
        }

        if self
            .daemon_status
            .as_ref()
            .map(|status| status.state == "ready")
            .unwrap_or(false)
            && !self.pending.contains("balance")
            && now.duration_since(self.last_balance_poll) > Duration::from_secs(6)
        {
            self.last_balance_poll = now;
            self.request_balance();
        }
    }

    fn render_top_bar(&mut self, ui: &mut Ui) {
        let status_text = if self.bridge.is_some() {
            "Local bridge online"
        } else {
            "Bridge unavailable"
        };
        let daemon_state = self
            .daemon_status
            .as_ref()
            .map(|status| status.state.to_uppercase())
            .unwrap_or_else(|| "STOPPED".to_owned());

        Frame::none()
            .fill(Color32::from_rgb(238, 229, 217))
            .inner_margin(egui::Margin::same(18.0))
            .rounding(egui::Rounding::same(18.0))
            .stroke(Stroke::new(1.0, Color32::from_rgb(216, 196, 173)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Kaspa Multisig Control Room")
                                .text_style(egui::TextStyle::Name("Hero".into()))
                                .color(INK),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Bootstrap with shared kpub exchange, sync through the local wallet daemon, then move transactions through unsigned -> partial -> fully signed -> broadcast.",
                            )
                            .color(Color32::from_rgb(86, 73, 63)),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        status_chip(ui, &daemon_state, state_color(self.daemon_status.as_ref().map(|state| state.state.as_str())));
                        status_chip(ui, status_text, if self.bridge.is_some() { TEAL } else { WARM_RED });
                    });
                });
            });
    }

    fn render_sidebar(&mut self, ui: &mut Ui) {
        side_card(ui, "Wallet Shape", Color32::from_rgb(251, 247, 241), |ui| {
            metric_line(
                ui,
                "Signer threshold",
                self.summary
                    .as_ref()
                    .map(|summary| {
                        format!(
                            "{} of {}",
                            summary.minimum_signatures, summary.public_key_count
                        )
                    })
                    .unwrap_or_else(|| {
                        format!(
                            "{} of {}",
                            self.bootstrap.minimum_signatures, self.bootstrap.num_public_keys
                        )
                    }),
            );
            metric_line(
                ui,
                "Cosigner index",
                self.summary
                    .as_ref()
                    .map(|summary| summary.cosigner_index.to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
            );
            metric_line(
                ui,
                "Fingerprint",
                self.summary
                    .as_ref()
                    .map(|summary| summary.fingerprint.clone())
                    .unwrap_or_else(|| "load wallet".to_owned()),
            );
            metric_line(
                ui,
                "Owned keys",
                self.summary
                    .as_ref()
                    .map(|summary| {
                        format!(
                            "{} local / {} total",
                            summary.owned_key_count, summary.public_key_count
                        )
                    })
                    .unwrap_or_else(|| {
                        format!(
                            "{} local / {} total",
                            self.bootstrap.num_private_keys, self.bootstrap.num_public_keys
                        )
                    }),
            );
            metric_line(
                ui,
                "Canonical receive owner",
                if self
                    .summary
                    .as_ref()
                    .map(|summary| summary.is_canonical_address_owner)
                    .unwrap_or(false)
                {
                    "yes".to_owned()
                } else {
                    "no".to_owned()
                },
            );
        });

        side_card(ui, "Daemon", Color32::from_rgb(244, 249, 249), |ui| {
            let state = self
                .daemon_status
                .as_ref()
                .map(|status| status.state.clone())
                .unwrap_or_else(|| "stopped".to_owned());
            let message = self
                .daemon_status
                .as_ref()
                .map(|status| status.message.clone())
                .unwrap_or_else(|| "No daemon process yet.".to_owned());
            ui.label(
                RichText::new(state.to_uppercase())
                    .strong()
                    .color(state_color(Some(state.as_str()))),
            );
            ui.add_space(6.0);
            ui.label(message);
            if let Some(status) = &self.daemon_status {
                if !status.daemon_address.is_empty() {
                    ui.add_space(8.0);
                    mono_value(ui, "Daemon address", &status.daemon_address);
                }
                if let Some(version) = &status.wallet_version {
                    mono_value(ui, "Wallet version", version);
                }
            }
        });

        side_card(ui, "Balance", Color32::from_rgb(248, 245, 236), |ui| {
            if let Some(balance) = &self.balance {
                metric_line(ui, "Available", format!("{} KAS", balance.available_kas));
                metric_line(ui, "Pending", format!("{} KAS", balance.pending_kas));
                metric_line(ui, "Live addresses", balance.addresses.len().to_string());
            } else {
                ui.label("Start and sync the daemon to load wallet balances.");
            }
        });

        side_card(ui, "Flow Rules", Color32::from_rgb(251, 245, 242), |ui| {
            ui.label("1. Bootstrap all cosigners with the same sorted kpub set.");
            ui.label("2. Only the canonical owner (index 0) should create receive addresses.");
            ui.label("3. Spending is always create unsigned -> sign -> sign -> broadcast.");
            ui.label("4. Large spends may produce multiple transactions; the backend already preserves that split/merge flow.");
        });
    }

    fn render_bootstrap_section(&mut self, ui: &mut Ui) {
        section_card(ui, "1. Bootstrap", "Create or recover the local cosigner file, exchange kpubs, and pin the shared wallet fingerprint.", COPPER, |ui| {
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    let previous_network = self.bootstrap.network;
                    ui.horizontal(|ui| {
                        ui.label("Network");
                        ComboBox::from_id_salt("bootstrap_network")
                            .selected_text(self.bootstrap.network.display_name())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.bootstrap.network, NetworkChoice::Mainnet, "Mainnet");
                                ui.selectable_value(&mut self.bootstrap.network, NetworkChoice::Testnet, "Testnet");
                                ui.selectable_value(&mut self.bootstrap.network, NetworkChoice::Devnet, "Devnet");
                                ui.selectable_value(&mut self.bootstrap.network, NetworkChoice::Simnet, "Simnet");
                            });
                        if self.bootstrap.network != previous_network {
                            self.bootstrap.sync_defaults();
                            self.daemon_form.sync_defaults(self.bootstrap.network);
                        }
                        if ui.button("Use guide defaults").clicked() {
                            self.bootstrap.sync_defaults();
                            self.daemon_form.sync_defaults(self.bootstrap.network);
                        }
                    });

                    ui.add_space(8.0);
                    field(ui, "Keys file", &mut self.bootstrap.keys_file);
                    password_field(ui, "Wallet password", &mut self.bootstrap.password);

                    ui.horizontal(|ui| {
                        numeric_drag(ui, "Min signatures", &mut self.bootstrap.minimum_signatures, 1..=16);
                        numeric_drag(ui, "Local keys", &mut self.bootstrap.num_private_keys, 1..=16);
                        numeric_drag(ui, "Total cosigners", &mut self.bootstrap.num_public_keys, 1..=16);
                    });
                    ui.checkbox(&mut self.bootstrap.ecdsa, "Use ECDSA wallet");
                    ui.checkbox(&mut self.bootstrap.overwrite, "Overwrite existing keys file");

                    let expected_remote = self.bootstrap.num_public_keys.saturating_sub(self.bootstrap.num_private_keys);
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Remote cosigner kpubs ({expected_remote} expected)")).strong());
                    ui.add(
                        TextEdit::multiline(&mut self.bootstrap.remote_public_keys)
                            .desired_rows(6)
                            .hint_text("Paste one kpub... per line."),
                    );

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.bootstrap.import_mode, "Recover from existing mnemonics");
                        if self.bootstrap.import_mode {
                            ui.label(RichText::new("One 24-word mnemonic per line").color(TEAL));
                        }
                    });
                    if self.bootstrap.import_mode {
                        ui.add(
                            TextEdit::multiline(&mut self.bootstrap.import_mnemonics)
                                .desired_rows(4)
                                .hint_text("word1 word2 ... word24"),
                        );
                    }
                });

                columns[1].vertical(|ui| {
                    ui.label(
                        RichText::new("Bootstrap validation")
                            .text_style(egui::TextStyle::Name("Section".into()))
                            .color(INK),
                    );
                    ui.add_space(6.0);
                    ui.label("The backend mirrors the Kaspa CLI flow:");
                    ui.label("Create/import local mnemonic(s) -> derive local kpub(s) -> append remote kpubs -> sort for multisig fingerprint -> save wallet file.");
                    ui.add_space(12.0);

                    if let Some(summary) = &self.summary {
                        metric_line(ui, "Fingerprint", summary.fingerprint.clone());
                        metric_line(ui, "Cosigner index", summary.cosigner_index.to_string());
                        metric_line(ui, "Canonical address owner", bool_word(summary.is_canonical_address_owner));
                        metric_line(ui, "Path", summary.keys_file.clone());
                    } else {
                        ui.label("Load an existing keys file or create a new one to populate the wallet summary.");
                    }

                    ui.add_space(14.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Load wallet summary").clicked() {
                            self.request_wallet_summary();
                        }
                        if ui.button("Create / recover wallet").clicked() {
                            self.request_wallet_create();
                        }
                        if ui.button("Reveal mnemonics + missing kpubs").clicked() {
                            self.request_export_secrets();
                        }
                    });
                });
            });

            if let Some(secrets) = &self.secrets {
                ui.add_space(14.0);
                secret_card(ui, secrets, &self.summary);
                ui.add_space(8.0);
                if ui.button("Clear secrets from screen").clicked() {
                    self.secrets = None;
                }
            }
        });
    }

    fn render_receive_section(&mut self, ui: &mut Ui) {
        section_card(ui, "2. Receive + Sync", "Run the Go wallet daemon locally, let it discover multisig branches, and only issue receive addresses from the canonical cosigner.", TEAL, |ui| {
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    field(ui, "RPC server", &mut self.daemon_form.rpc_server);
                    field(ui, "Daemon listen (optional)", &mut self.daemon_form.listen);
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Start local daemon").clicked() {
                            self.request_start_daemon();
                        }
                        if ui.button("Stop daemon").clicked() {
                            self.request_stop_daemon();
                        }
                        if ui.button("Refresh status").clicked() {
                            self.request_daemon_status();
                        }
                        if ui.button("Refresh balance").clicked() {
                            self.request_balance();
                        }
                    });

                    ui.add_space(10.0);
                    let can_issue_receive = self
                        .summary
                        .as_ref()
                        .map(|summary| !summary.is_multisig || summary.is_canonical_address_owner)
                        .unwrap_or(true);
                    ui.add_enabled_ui(can_issue_receive, |ui| {
                        if ui.button("Create canonical receive address").clicked() {
                            self.request_new_address();
                        }
                    });
                    if !can_issue_receive {
                        ui.label(RichText::new("This cosigner is not index 0. Use the canonical owner for receive addresses.").color(WARM_RED));
                    }
                    if ui.button("List known receive addresses").clicked() {
                        self.request_addresses();
                    }
                });

                columns[1].vertical(|ui| {
                    ui.label(RichText::new("Live sync state").strong());
                    if let Some(status) = &self.daemon_status {
                        ui.label(RichText::new(status.state.to_uppercase()).color(state_color(Some(status.state.as_str()))).strong());
                        ui.label(status.message.clone());
                        if let Some(started_at) = &status.started_at {
                            metric_line(ui, "Started", started_at.clone());
                        }
                        if !status.daemon_address.is_empty() {
                            mono_value(ui, "Daemon", &status.daemon_address);
                        }
                    } else {
                        ui.label("No daemon status yet.");
                    }

                    ui.add_space(12.0);
                    if let Some(address) = &self.last_new_address {
                        mono_value(ui, "Latest receive address", address);
                    }
                    if !self.addresses.is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Known receive addresses").strong());
                        for address in self.addresses.iter().take(6) {
                            mono_line(ui, address);
                        }
                    }
                });
            });
        });
    }

    fn render_spend_section(&mut self, ui: &mut Ui) {
        section_card(ui, "3. Spend Pipeline", "The Go backend preserves the real multisig spend flow: create unsigned transactions, collect signatures offline, then broadcast once the bundle is fully signed.", OLIVE, |ui| {
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    field(ui, "Destination", &mut self.spend.to_address);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.spend.send_all, "Send all");
                        if !self.spend.send_all {
                            field(ui, "Amount (KAS)", &mut self.spend.amount_kas);
                        }
                    });
                    ui.checkbox(&mut self.spend.use_existing_change_address, "Prefer an existing change address");
                    ui.label(RichText::new("From addresses (optional)").strong());
                    ui.add(
                        TextEdit::multiline(&mut self.spend.from_addresses)
                            .desired_rows(3)
                            .hint_text("Filter UTXOs by wallet address. One per line."),
                    );

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label("Fee policy");
                        ComboBox::from_id_salt("fee_mode")
                            .selected_text(self.spend.fee_mode.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.spend.fee_mode, FeeMode::Estimate, FeeMode::Estimate.label());
                                ui.selectable_value(&mut self.spend.fee_mode, FeeMode::ExactFeeRate, FeeMode::ExactFeeRate.label());
                                ui.selectable_value(&mut self.spend.fee_mode, FeeMode::MaxFeeRate, FeeMode::MaxFeeRate.label());
                                ui.selectable_value(&mut self.spend.fee_mode, FeeMode::MaxFee, FeeMode::MaxFee.label());
                            });
                    });
                    if self.spend.fee_mode != FeeMode::Estimate {
                        field(ui, "Fee value", &mut self.spend.fee_value);
                    }

                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Create unsigned").clicked() {
                            self.request_create_unsigned();
                        }
                        if ui.button("Sign current hex").clicked() {
                            self.request_sign_flow();
                        }
                        if ui.button("Parse current hex").clicked() {
                            self.request_parse_flow();
                        }
                        if ui.button("Broadcast current hex").clicked() {
                            self.request_broadcast_flow();
                        }
                    });
                });

                columns[1].vertical(|ui| {
                    let stage = flow_stage(self.flow_bundle.as_ref());
                    status_chip(ui, stage.0, stage.1);
                    if !self.last_broadcast_tx_ids.is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Last broadcast txids").strong());
                        for txid in &self.last_broadcast_tx_ids {
                            mono_line(ui, txid);
                        }
                    }
                });
            });

            ui.add_space(12.0);
            ui.label(RichText::new("Transaction bundle hex").strong());
            ui.add(
                TextEdit::multiline(&mut self.flow_hex)
                    .desired_rows(8)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("Unsigned, partially signed, or fully signed bundle hex."),
            );
            ui.horizontal(|ui| {
                if ui.button("Copy bundle hex").clicked() {
                    copy_text(ui, self.flow_hex.clone());
                }
                if ui.button("Use bundle hex in inspector").clicked() {
                    self.inspector.transactions_hex = self.flow_hex.clone();
                }
                if ui.button("Clear bundle").clicked() {
                    self.flow_hex.clear();
                    self.flow_bundle = None;
                    self.last_broadcast_tx_ids.clear();
                }
            });

            if let Some(bundle) = &self.flow_bundle {
                ui.add_space(14.0);
                render_transaction_bundle(ui, bundle);
            }
        });
    }

    fn render_inspector_section(&mut self, ui: &mut Ui) {
        section_card(ui, "4. Inspector", "Review any transaction bundle offline. This is useful for audit logs, signature collection, and final broadcast checks.", WARM_RED, |ui| {
            ui.add(
                TextEdit::multiline(&mut self.inspector.transactions_hex)
                    .desired_rows(8)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("Paste any bundle hex here for offline inspection."),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("Parse inspector hex").clicked() {
                    self.request_parse_inspector();
                }
                if ui.button("Pull current flow hex").clicked() {
                    self.inspector.transactions_hex = self.flow_hex.clone();
                }
                if ui.button("Copy inspector hex").clicked() {
                    copy_text(ui, self.inspector.transactions_hex.clone());
                }
            });

            if let Some(bundle) = &self.inspector_bundle {
                ui.add_space(14.0);
                render_transaction_bundle(ui, bundle);
            }
        });
    }
}

impl eframe::App for WalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        self.auto_poll();
        ctx.request_repaint_after(Duration::from_millis(250));

        egui::TopBottomPanel::top("top_bar")
            .frame(
                Frame::none()
                    .fill(SAND)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                self.render_top_bar(ui);
                if let Some(banner) = &self.banner {
                    ui.add_space(8.0);
                    banner_line(ui, banner);
                }
            });

        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(290.0)
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(242, 236, 228))
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_sidebar(ui);
                });
            });

        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(CREAM)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_bootstrap_section(ui);
                    ui.add_space(12.0);
                    self.render_receive_section(ui);
                    ui.add_space(12.0);
                    self.render_spend_section(ui);
                    ui.add_space(12.0);
                    self.render_inspector_section(ui);
                });
            });
    }
}

fn section_card<R>(
    ui: &mut Ui,
    title: &str,
    subtitle: &str,
    accent: Color32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    Frame::none()
        .fill(Color32::from_rgb(255, 252, 248))
        .inner_margin(egui::Margin::same(18.0))
        .rounding(egui::Rounding::same(18.0))
        .stroke(Stroke::new(1.0, Color32::from_rgb(215, 200, 186)))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(title)
                        .text_style(egui::TextStyle::Name("Section".into()))
                        .color(INK),
                );
                status_chip(ui, "multisig", accent);
            });
            ui.add_space(4.0);
            ui.label(RichText::new(subtitle).color(Color32::from_rgb(92, 80, 72)));
            ui.add_space(14.0);
            add_contents(ui)
        })
        .inner
}

fn side_card<R>(
    ui: &mut Ui,
    title: &str,
    fill: Color32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    Frame::none()
        .fill(fill)
        .inner_margin(egui::Margin::same(14.0))
        .rounding(egui::Rounding::same(16.0))
        .stroke(Stroke::new(1.0, Color32::from_rgb(215, 200, 186)))
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong().color(INK));
            ui.add_space(8.0);
            add_contents(ui)
        })
        .inner
}

fn secret_card(ui: &mut Ui, secrets: &ExportSecretsResponse, summary: &Option<WalletSummary>) {
    Frame::none()
        .fill(Color32::from_rgb(255, 245, 236))
        .inner_margin(egui::Margin::same(14.0))
        .rounding(egui::Rounding::same(14.0))
        .stroke(Stroke::new(1.0, Color32::from_rgb(224, 176, 131)))
        .show(ui, |ui| {
            ui.label(RichText::new("Sensitive material on screen").strong().color(WARM_RED));
            ui.label("Treat the following as temporary. Mnemonics should be written down offline, and only kpub strings are safe to share with cosigners.");
            ui.add_space(8.0);

            if !secrets.mnemonics.is_empty() {
                ui.label(RichText::new("Local mnemonics").strong());
                for (index, mnemonic) in secrets.mnemonics.iter().enumerate() {
                    mono_value(ui, &format!("Mnemonic #{}", index + 1), mnemonic);
                }
            }

            let local_pub_keys: Vec<String> = if let Some(summary) = summary {
                summary
                    .public_keys
                    .iter()
                    .take(summary.owned_key_count)
                    .cloned()
                    .collect()
            } else {
                secrets.external_public_keys.clone()
            };
            if !local_pub_keys.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Local kpub strings").strong());
                for (index, public_key) in local_pub_keys.iter().enumerate() {
                    mono_value(ui, &format!("kpub #{}", index + 1), public_key);
                }
            }

            if !secrets.external_public_keys.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("External-only cosigner kpubs").strong());
                for public_key in &secrets.external_public_keys {
                    mono_line(ui, public_key);
                }
            }
        });
}

fn render_transaction_bundle(ui: &mut Ui, bundle: &TransactionBundle) {
    ui.label(
        RichText::new(format!(
            "{} transaction(s) in bundle{}",
            bundle.transaction_count,
            if bundle.fully_signed {
                ", ready to broadcast"
            } else {
                ""
            }
        ))
        .strong(),
    );
    ui.add_space(8.0);

    for transaction in &bundle.transactions {
        Frame::none()
            .fill(Color32::from_rgb(250, 247, 243))
            .inner_margin(egui::Margin::same(12.0))
            .rounding(egui::Rounding::same(14.0))
            .stroke(Stroke::new(1.0, Color32::from_rgb(220, 206, 191)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Transaction #{}", transaction.index)).strong());
                    status_chip(
                        ui,
                        if transaction.fully_signed {
                            "fully signed"
                        } else {
                            "needs signatures"
                        },
                        if transaction.fully_signed {
                            OLIVE
                        } else {
                            COPPER
                        },
                    );
                });
                mono_value(ui, "TxID", &transaction.tx_id);
                ui.add_space(6.0);
                metric_line(
                    ui,
                    "Shape",
                    format!(
                        "{} inputs / {} outputs",
                        transaction.input_count, transaction.output_count
                    ),
                );
                metric_line(ui, "Fee", format!("{} KAS", transaction.fee_kas));
                if transaction.has_mass_estimate {
                    metric_line(ui, "Mass", transaction.mass.to_string());
                    metric_line(
                        ui,
                        "Fee rate",
                        format!("{:.2} Sompi/gram", transaction.fee_rate),
                    );
                }

                ui.add_space(8.0);
                ui.label(RichText::new("Signature progress").strong());
                for progress in &transaction.signatures {
                    ui.label(format!(
                        "Input {}: {} of {} signatures collected",
                        progress.input_index, progress.signed_by, progress.minimum_signatures
                    ));
                }

                ui.add_space(8.0);
                ui.label(RichText::new("Outputs").strong());
                for output in &transaction.outputs {
                    mono_value(ui, &output.amount_kas, &output.address);
                }
            });
        ui.add_space(10.0);
    }
}

fn banner_line(ui: &mut Ui, banner: &BannerMessage) {
    let (fill, text) = match banner.tone {
        MessageTone::Info => (Color32::from_rgb(236, 247, 245), TEAL),
        MessageTone::Error => (Color32::from_rgb(254, 240, 238), WARM_RED),
    };
    Frame::none()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .rounding(egui::Rounding::same(12.0))
        .stroke(Stroke::new(1.0, text))
        .show(ui, |ui| {
            ui.label(RichText::new(&banner.text).color(text));
        });
}

fn status_chip(ui: &mut Ui, label: impl AsRef<str>, color: Color32) {
    Frame::none()
        .fill(color.gamma_multiply(0.12))
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .rounding(egui::Rounding::same(999.0))
        .stroke(Stroke::new(1.0, color))
        .show(ui, |ui| {
            ui.label(RichText::new(label.as_ref()).color(color).strong());
        });
}

fn metric_line(ui: &mut Ui, label: &str, value: String) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(label).color(Color32::from_rgb(110, 97, 86)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).strong().color(INK));
        });
    });
}

fn mono_value(ui: &mut Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).color(Color32::from_rgb(111, 97, 86)));
    mono_line(ui, value);
}

fn mono_line(ui: &mut Ui, value: &str) {
    ui.label(RichText::new(value).monospace().color(INK));
}

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).strong());
    ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn password_field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).strong());
    ui.add(
        TextEdit::singleline(value)
            .password(true)
            .desired_width(f32::INFINITY),
    );
}

fn numeric_drag(ui: &mut Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).strong());
        ui.add(egui::DragValue::new(value).range(range));
    });
}

fn split_lines(input: &str) -> Vec<String> {
    input
        .split(|ch| ch == '\n' || ch == ',')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_f64_field(name: &str, value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("invalid {name}: {value}"))
}

fn parse_u64_field(name: &str, value: &str) -> Result<u64, String> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("invalid {name}: {value}"))
}

fn bool_word(value: bool) -> String {
    if value {
        "yes".to_owned()
    } else {
        "no".to_owned()
    }
}

fn state_color(state: Option<&str>) -> Color32 {
    match state.unwrap_or("stopped") {
        "ready" => OLIVE,
        "syncing" | "running" | "starting" => COPPER,
        _ => WARM_RED,
    }
}

fn flow_stage(bundle: Option<&TransactionBundle>) -> (&'static str, Color32) {
    match bundle {
        None => ("awaiting bundle", Color32::from_rgb(118, 110, 102)),
        Some(bundle) if bundle.fully_signed => ("ready to broadcast", OLIVE),
        Some(bundle)
            if bundle
                .transactions
                .iter()
                .flat_map(|tx| tx.signatures.iter())
                .any(|sig| sig.signed_by > 0) =>
        {
            ("partially signed", COPPER)
        }
        Some(_) => ("unsigned draft", TEAL),
    }
}

fn copy_text(ui: &Ui, text: String) {
    ui.output_mut(|output| {
        output.copied_text = text;
    });
}
