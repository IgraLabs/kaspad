mod backend;
mod theme;

use backend::{
    AddressesResponse, BackendClient, BackendProcess, BalanceResponse, BootstrapCreateRequest,
    BootstrapCreateResponse, BroadcastRequest, BroadcastResponse, CreateUnsignedRequest,
    DaemonStatus, ExportSecretsRequest, ExportSecretsResponse, FeePolicyRequest,
    NewAddressResponse, ParseRequest, SessionConfigRequest, SignRequest, TransactionBundle,
    WalletSummary, WalletSummaryRequest,
};
use eframe::egui::{
    self, Align, Color32, ComboBox, Frame, Label, Layout, RichText, Stroke, TextEdit, Ui,
};
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

const INK: Color32 = Color32::from_rgb(234, 255, 250);
const TEXT_SOFT: Color32 = Color32::from_rgb(143, 191, 182);
const COPPER: Color32 = Color32::from_rgb(58, 221, 190);
const TEAL: Color32 = Color32::from_rgb(73, 234, 203);
const SAND: Color32 = Color32::from_rgb(6, 22, 24);
const CREAM: Color32 = Color32::from_rgb(10, 33, 36);
const PANEL_ALT: Color32 = Color32::from_rgb(13, 44, 48);
const PANEL_SOFT: Color32 = Color32::from_rgb(16, 56, 60);
const STROKE: Color32 = Color32::from_rgb(31, 92, 90);
const WARM_RED: Color32 = Color32::from_rgb(255, 122, 136);
const OLIVE: Color32 = Color32::from_rgb(112, 199, 186);

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1040.0, 720.0])
            .with_title("Kaspa Multisig Control Room"),
        ..Default::default()
    };

    eframe::run_native(
        "Kaspa Multisig Control Room",
        native_options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(WalletApp::new(&cc.egui_ctx)))
        }),
    )
}

fn load_logo_texture(ctx: &egui::Context) -> Result<egui::TextureHandle, String> {
    let bytes = include_bytes!("../assets/igralabs-logo.png");
    let image = image::load_from_memory(bytes)
        .map_err(|err| format!("failed to decode the IgraLabs logo: {err}"))?
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Ok(ctx.load_texture("igralabs-logo", color_image, egui::TextureOptions::LINEAR))
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

struct NodeForm {
    rpc_server: String,
}

impl NodeForm {
    fn new(network: NetworkChoice) -> Self {
        Self {
            rpc_server: network.default_rpc_server().to_owned(),
        }
    }

    fn sync_defaults(&mut self, network: NetworkChoice) {
        self.rpc_server = network.default_rpc_server().to_owned();
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
    logo_texture: Option<egui::TextureHandle>,
    bootstrap: BootstrapForm,
    node_form: NodeForm,
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
    fn new(ctx: &egui::Context) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let bootstrap = BootstrapForm::new();
        let node_form = NodeForm::new(bootstrap.network);
        let logo_texture = load_logo_texture(ctx).ok();

        let (backend, bridge, banner) = match BackendProcess::spawn() {
            Ok(process) => {
                let bridge = process.client();
                (
                    Some(process),
                    Some(bridge),
                    Some(BannerMessage {
                        tone: MessageTone::Info,
                        text: "Local Go wallet bridge is running. Load or create a multisig wallet, then point it at a Kaspa node RPC.".to_owned(),
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
            logo_texture,
            bootstrap,
            node_form,
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

    fn current_session_request(&self) -> Result<SessionConfigRequest, String> {
        let rpc_server = self.node_form.rpc_server.trim();
        if rpc_server.is_empty() {
            return Err("Kaspa node RPC is required for sync and spend operations.".to_owned());
        }
        Ok(SessionConfigRequest {
            network: self.network_string(),
            keys_file: self.bootstrap.keys_file.clone(),
            rpc_server: rpc_server.to_owned(),
            timeout_seconds: 30,
        })
    }

    fn maybe_connect_node(&mut self) {
        if self.node_form.rpc_server.trim().is_empty() || self.pending.contains("session_config") {
            return;
        }
        self.request_connect_node();
    }

    fn request_connect_node(&mut self) {
        let request = match self.current_session_request() {
            Ok(request) => request,
            Err(err) => {
                self.set_banner(MessageTone::Error, err);
                return;
            }
        };
        self.spawn_task("session_config", move |bridge| {
            AppEvent::DaemonStatus(bridge.configure_session(&request))
        });
    }

    fn request_daemon_status(&mut self) {
        self.spawn_task("daemon_status", move |bridge| {
            AppEvent::DaemonStatus(bridge.daemon_status())
        });
    }

    fn request_balance(&mut self) {
        let session = match self.current_session_request() {
            Ok(request) => request,
            Err(err) => {
                self.set_banner(MessageTone::Error, err);
                return;
            }
        };
        self.spawn_task("balance", move |bridge| {
            if let Err(err) = bridge.configure_session(&session) {
                return AppEvent::Balance(Err(err));
            }
            AppEvent::Balance(bridge.balance())
        });
    }

    fn request_addresses(&mut self) {
        let session = match self.current_session_request() {
            Ok(request) => request,
            Err(err) => {
                self.set_banner(MessageTone::Error, err);
                return;
            }
        };
        self.spawn_task("addresses", move |bridge| {
            if let Err(err) = bridge.configure_session(&session) {
                return AppEvent::Addresses(Err(err));
            }
            AppEvent::Addresses(bridge.list_addresses())
        });
    }

    fn request_new_address(&mut self) {
        let session = match self.current_session_request() {
            Ok(request) => request,
            Err(err) => {
                self.set_banner(MessageTone::Error, err);
                return;
            }
        };
        self.spawn_task("new_address", move |bridge| {
            if let Err(err) = bridge.configure_session(&session) {
                return AppEvent::NewAddress(Err(err));
            }
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
        let session = match self.current_session_request() {
            Ok(request) => request,
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
            if let Err(err) = bridge.configure_session(&session) {
                return AppEvent::FlowBundle(Err(err));
            }
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
        let session = match self.current_session_request() {
            Ok(request) => request,
            Err(err) => {
                self.set_banner(MessageTone::Error, err);
                return;
            }
        };
        let request = BroadcastRequest {
            transactions_hex: self.flow_hex.clone(),
        };
        self.spawn_task("broadcast", move |bridge| {
            if let Err(err) = bridge.configure_session(&session) {
                return AppEvent::Broadcasted(Err(err));
            }
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
                            self.bootstrap.keys_file = summary.keys_file.clone();
                            self.summary = Some(summary);
                            self.set_banner(
                                MessageTone::Info,
                                "Loaded wallet summary from the Go backend.",
                            );
                            self.maybe_connect_node();
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
                            self.maybe_connect_node();
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
                    self.pending.remove("session_config");
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
                            if !self.pending.contains("daemon_status") {
                                self.request_daemon_status();
                            }
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::Addresses(result) => {
                    self.pending.remove("addresses");
                    match result {
                        Ok(addresses) => {
                            self.addresses = addresses.addresses;
                            if !self.pending.contains("daemon_status") {
                                self.request_daemon_status();
                            }
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
                            if !self.pending.contains("daemon_status") {
                                self.request_daemon_status();
                            }
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
                            if !self.pending.contains("daemon_status") {
                                self.request_daemon_status();
                            }
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
            "Bridge online"
        } else {
            "Bridge offline"
        };
        let daemon_state = self
            .daemon_status
            .as_ref()
            .map(|status| status.state.to_uppercase())
            .unwrap_or_else(|| "STOPPED".to_owned());

        Frame::none()
            .fill(PANEL_ALT)
            .inner_margin(egui::Margin::same(18.0))
            .rounding(egui::Rounding::same(18.0))
            .stroke(Stroke::new(1.0, STROKE))
            .show(ui, |ui| {
                let compact = ui.available_width() < 1120.0;

                if compact {
                    ui.vertical(|ui| {
                        render_header_title(ui);
                        supporting_text(ui, "Create or import cosigners, connect to a Kaspa node, and move multisig bundles from unsigned to final broadcast.");
                        ui.add_space(10.0);
                        render_brand_lockup(ui, self.logo_texture.as_ref(), true);
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            status_chip(
                                ui,
                                status_text,
                                if self.bridge.is_some() { TEAL } else { WARM_RED },
                            );
                            status_chip(
                                ui,
                                &daemon_state,
                                state_color(
                                    self.daemon_status
                                        .as_ref()
                                        .map(|state| state.state.as_str()),
                                ),
                            );
                        });
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            render_header_title(ui);
                            supporting_text(ui, "Create or import cosigners, connect to a Kaspa node, and move multisig bundles from unsigned to final broadcast.");
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            render_brand_lockup(ui, self.logo_texture.as_ref(), false);
                        });
                    });
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        status_chip(
                            ui,
                            status_text,
                            if self.bridge.is_some() { TEAL } else { WARM_RED },
                        );
                        status_chip(
                            ui,
                            &daemon_state,
                            state_color(
                                self.daemon_status
                                    .as_ref()
                                    .map(|state| state.state.as_str()),
                            ),
                        );
                    });
                }
            });
    }

    fn render_sidebar(&mut self, ui: &mut Ui) {
        side_card(ui, "Wallet Shape", CREAM, |ui| {
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

        side_card(ui, "Wallet Engine", PANEL_ALT, |ui| {
            let state = self
                .daemon_status
                .as_ref()
                .map(|status| status.state.clone())
                .unwrap_or_else(|| "stopped".to_owned());
            let message = self
                .daemon_status
                .as_ref()
                .map(|status| status.message.clone())
                .unwrap_or_else(|| {
                    "The app will start the internal wallet engine when a node RPC is configured."
                        .to_owned()
                });
            ui.label(
                RichText::new(state.to_uppercase())
                    .strong()
                    .color(state_color(Some(state.as_str()))),
            );
            ui.add_space(6.0);
            ui.label(message);
            if let Some(status) = &self.daemon_status {
                if !status.rpc_server.is_empty() {
                    ui.add_space(8.0);
                    mono_value(ui, "Node RPC", &status.rpc_server);
                }
                if let Some(version) = &status.wallet_version {
                    mono_value(ui, "Wallet version", version);
                }
            }
        });

        side_card(ui, "Balance", CREAM, |ui| {
            if let Some(balance) = &self.balance {
                metric_line(ui, "Available", format!("{} KAS", balance.available_kas));
                metric_line(ui, "Pending", format!("{} KAS", balance.pending_kas));
                metric_line(ui, "Live addresses", balance.addresses.len().to_string());
            } else {
                ui.label("Connect the wallet to a node RPC to load live balances.");
            }
        });

        side_card(ui, "Flow Rules", PANEL_ALT, |ui| {
            wrapped_text(
                ui,
                "1. Bootstrap all cosigners with the same sorted kpub set.",
                INK,
            );
            wrapped_text(
                ui,
                "2. Only the canonical owner (index 0) should create receive addresses.",
                INK,
            );
            wrapped_text(
                ui,
                "3. Spending is always create unsigned -> sign -> sign -> broadcast.",
                INK,
            );
            wrapped_text(
                ui,
                "4. The app manages the internal wallet engine and restarts it if the node RPC changes.",
                INK,
            );
            wrapped_text(
                ui,
                "5. Large spends may produce multiple transactions; the backend already preserves that split/merge flow.",
                INK,
            );
        });
    }

    fn render_bootstrap_editor(&mut self, ui: &mut Ui) {
        let previous_network = self.bootstrap.network;
        ui.horizontal_wrapped(|ui| {
            ui.label("Network");
            ComboBox::from_id_salt("bootstrap_network")
                .selected_text(self.bootstrap.network.display_name())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.bootstrap.network,
                        NetworkChoice::Mainnet,
                        "Mainnet",
                    );
                    ui.selectable_value(
                        &mut self.bootstrap.network,
                        NetworkChoice::Testnet,
                        "Testnet",
                    );
                    ui.selectable_value(
                        &mut self.bootstrap.network,
                        NetworkChoice::Devnet,
                        "Devnet",
                    );
                    ui.selectable_value(
                        &mut self.bootstrap.network,
                        NetworkChoice::Simnet,
                        "Simnet",
                    );
                });
            if self.bootstrap.network != previous_network {
                self.bootstrap.sync_defaults();
                self.node_form.sync_defaults(self.bootstrap.network);
            }
            if ui.button("Use guide defaults").clicked() {
                self.bootstrap.sync_defaults();
                self.node_form.sync_defaults(self.bootstrap.network);
            }
        });

        ui.add_space(8.0);
        field(ui, "Keys file", &mut self.bootstrap.keys_file);
        password_field(ui, "Wallet password", &mut self.bootstrap.password);

        ui.horizontal_wrapped(|ui| {
            numeric_drag(
                ui,
                "Min signatures",
                &mut self.bootstrap.minimum_signatures,
                1..=16,
            );
            numeric_drag(
                ui,
                "Local keys",
                &mut self.bootstrap.num_private_keys,
                1..=16,
            );
            numeric_drag(
                ui,
                "Total cosigners",
                &mut self.bootstrap.num_public_keys,
                1..=16,
            );
        });
        ui.checkbox(&mut self.bootstrap.ecdsa, "Use ECDSA wallet");
        ui.checkbox(
            &mut self.bootstrap.overwrite,
            "Overwrite existing keys file",
        );

        let expected_remote = self
            .bootstrap
            .num_public_keys
            .saturating_sub(self.bootstrap.num_private_keys);
        ui.add_space(10.0);
        ui.label(
            RichText::new(format!(
                "Remote cosigner kpubs ({expected_remote} expected)"
            ))
            .strong(),
        );
        ui.add(
            TextEdit::multiline(&mut self.bootstrap.remote_public_keys)
                .desired_rows(6)
                .hint_text("Paste one kpub... per line."),
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.bootstrap.import_mode,
                "Recover from existing mnemonics",
            );
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
    }

    fn render_bootstrap_summary_panel(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new("Bootstrap validation")
                .text_style(egui::TextStyle::Name("Section".into()))
                .color(INK),
        );
        ui.add_space(6.0);
        supporting_text(ui, "The backend mirrors the Kaspa CLI multisig flow.");
        supporting_text(
            ui,
            "Create or import local mnemonic(s), derive local kpub(s), append remote kpubs, sort them for the shared fingerprint, then save the wallet file.",
        );
        ui.add_space(12.0);

        if let Some(summary) = &self.summary {
            metric_line(ui, "Fingerprint", summary.fingerprint.clone());
            metric_line(ui, "Cosigner index", summary.cosigner_index.to_string());
            metric_line(
                ui,
                "Canonical address owner",
                bool_word(summary.is_canonical_address_owner),
            );
            metric_line(ui, "Path", summary.keys_file.clone());
        } else {
            supporting_text(
                ui,
                "Load an existing keys file or create a new one to populate the wallet summary.",
            );
        }

        ui.add_space(14.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Load wallet summary").clicked() {
                self.request_wallet_summary();
            }
            if ui.button("Create or recover wallet").clicked() {
                self.request_wallet_create();
            }
            if ui.button("Reveal recovery material").clicked() {
                self.request_export_secrets();
            }
        });
    }

    fn render_receive_controls(&mut self, ui: &mut Ui) {
        field(ui, "Kaspa node RPC", &mut self.node_form.rpc_server);
        supporting_text(
            ui,
            "The wallet engine starts internally and restarts automatically when this endpoint changes.",
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Connect to node").clicked() {
                self.request_connect_node();
            }
            if ui.button("Check sync status").clicked() {
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
            if ui.button("Create receive address").clicked() {
                self.request_new_address();
            }
        });
        if !can_issue_receive {
            wrapped_text(
                ui,
                "This cosigner is not index 0. Use the canonical owner for receive addresses.",
                WARM_RED,
            );
        }
        if ui.button("Show known addresses").clicked() {
            self.request_addresses();
        }
    }

    fn render_receive_status_panel(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Live sync state").strong());
        if let Some(status) = &self.daemon_status {
            ui.label(
                RichText::new(status.state.to_uppercase())
                    .color(state_color(Some(status.state.as_str())))
                    .strong(),
            );
            wrapped_text(ui, &status.message, INK);
            if let Some(started_at) = &status.started_at {
                metric_line(ui, "Started", started_at.clone());
            }
            if !status.rpc_server.is_empty() {
                mono_value(ui, "Node RPC", &status.rpc_server);
            }
            if let Some(version) = &status.wallet_version {
                mono_value(ui, "Wallet version", version);
            }
        } else {
            supporting_text(ui, "Provide a node RPC to start syncing.");
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
    }

    fn render_spend_controls(&mut self, ui: &mut Ui) {
        field(ui, "Destination", &mut self.spend.to_address);
        if ui.available_width() < 520.0 {
            ui.vertical(|ui| {
                ui.checkbox(&mut self.spend.send_all, "Send all");
                if !self.spend.send_all {
                    field(ui, "Amount (KAS)", &mut self.spend.amount_kas);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.spend.send_all, "Send all");
                if !self.spend.send_all {
                    field(ui, "Amount (KAS)", &mut self.spend.amount_kas);
                }
            });
        }
        ui.checkbox(
            &mut self.spend.use_existing_change_address,
            "Prefer an existing change address",
        );
        ui.label(RichText::new("From addresses (optional)").strong());
        supporting_text(
            ui,
            "Filter UTXOs by wallet address. Leave blank to spend from the full wallet set.",
        );
        ui.add(
            TextEdit::multiline(&mut self.spend.from_addresses)
                .desired_rows(3)
                .hint_text("one address per line"),
        );

        ui.add_space(10.0);
        if ui.available_width() < 520.0 {
            ui.vertical(|ui| {
                ui.label("Fee policy");
                ComboBox::from_id_salt("fee_mode")
                    .selected_text(self.spend.fee_mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::Estimate,
                            FeeMode::Estimate.label(),
                        );
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::ExactFeeRate,
                            FeeMode::ExactFeeRate.label(),
                        );
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::MaxFeeRate,
                            FeeMode::MaxFeeRate.label(),
                        );
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::MaxFee,
                            FeeMode::MaxFee.label(),
                        );
                    });
            });
        } else {
            ui.horizontal(|ui| {
                ui.label("Fee policy");
                ComboBox::from_id_salt("fee_mode")
                    .selected_text(self.spend.fee_mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::Estimate,
                            FeeMode::Estimate.label(),
                        );
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::ExactFeeRate,
                            FeeMode::ExactFeeRate.label(),
                        );
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::MaxFeeRate,
                            FeeMode::MaxFeeRate.label(),
                        );
                        ui.selectable_value(
                            &mut self.spend.fee_mode,
                            FeeMode::MaxFee,
                            FeeMode::MaxFee.label(),
                        );
                    });
            });
        }
        if self.spend.fee_mode != FeeMode::Estimate {
            field(ui, "Fee value", &mut self.spend.fee_value);
        }

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Create unsigned").clicked() {
                self.request_create_unsigned();
            }
            if ui.button("Add local signature").clicked() {
                self.request_sign_flow();
            }
            if ui.button("Inspect bundle").clicked() {
                self.request_parse_flow();
            }
            if ui.button("Broadcast bundle").clicked() {
                self.request_broadcast_flow();
            }
        });
    }

    fn render_spend_status_panel(&mut self, ui: &mut Ui) {
        let stage = flow_stage(self.flow_bundle.as_ref());
        status_chip(ui, stage.0, stage.1);
        if !self.last_broadcast_tx_ids.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("Last broadcast txids").strong());
            for txid in &self.last_broadcast_tx_ids {
                mono_line(ui, txid);
            }
        }
    }

    fn render_bootstrap_section(&mut self, ui: &mut Ui) {
        section_card(ui, "1. Bootstrap", "Create or recover the local cosigner file, exchange kpubs, and lock the shared wallet fingerprint.", COPPER, |ui| {
            if ui.available_width() >= 980.0 {
                ui.columns(2, |columns| {
                    self.render_bootstrap_editor(&mut columns[0]);
                    self.render_bootstrap_summary_panel(&mut columns[1]);
                });
            } else {
                self.render_bootstrap_editor(ui);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                self.render_bootstrap_summary_panel(ui);
            }

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
        section_card(ui, "2. Receive + Sync", "Provide a node RPC. The app manages the internal wallet engine and keeps receive flow on the canonical cosigner.", TEAL, |ui| {
            if ui.available_width() >= 960.0 {
                ui.columns(2, |columns| {
                    self.render_receive_controls(&mut columns[0]);
                    self.render_receive_status_panel(&mut columns[1]);
                });
            } else {
                self.render_receive_controls(ui);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                self.render_receive_status_panel(ui);
            }
        });
    }

    fn render_spend_section(&mut self, ui: &mut Ui) {
        section_card(ui, "3. Spend Pipeline", "The Go backend preserves the real multisig spend flow: create unsigned transactions, collect signatures offline, then broadcast once the bundle is fully signed.", OLIVE, |ui| {
            if ui.available_width() >= 1040.0 {
                ui.columns(2, |columns| {
                    self.render_spend_controls(&mut columns[0]);
                    self.render_spend_status_panel(&mut columns[1]);
                });
            } else {
                self.render_spend_controls(ui);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                self.render_spend_status_panel(ui);
            }

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
        section_card(ui, "4. Inspector", "Review any transaction bundle offline. This is useful for audit logs, signature collection, and final broadcast checks.", OLIVE, |ui| {
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
                if ui.button("Use current bundle").clicked() {
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
                    .fill(SAND)
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
                    .fill(SAND)
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
        .fill(CREAM)
        .inner_margin(egui::Margin::same(18.0))
        .rounding(egui::Rounding::same(18.0))
        .stroke(Stroke::new(1.0, STROKE))
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
            supporting_text(ui, subtitle);
            ui.add_space(14.0);
            add_contents(ui)
        })
        .inner
}

fn render_header_title(ui: &mut Ui) {
    ui.label(
        RichText::new("Kaspa Multisig Control Room")
            .text_style(egui::TextStyle::Name("Hero".into()))
            .color(INK),
    );
}

fn render_brand_lockup(ui: &mut Ui, logo_texture: Option<&egui::TextureHandle>, compact: bool) {
    let logo_size = if compact {
        egui::vec2(188.0, 60.0)
    } else {
        egui::vec2(212.0, 68.0)
    };

    Frame::none()
        .fill(Color32::from_rgb(238, 247, 245))
        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
        .rounding(egui::Rounding::same(16.0))
        .stroke(Stroke::new(1.0, Color32::from_rgb(170, 210, 203)))
        .show(ui, |ui| {
            if let Some(texture) = logo_texture {
                ui.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(logo_size)
                        .maintain_aspect_ratio(true),
                );
            } else {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("IGRA LABS")
                            .strong()
                            .color(Color32::from_rgb(9, 35, 37))
                            .text_style(egui::TextStyle::Name("Section".into())),
                    );
                    ui.label(
                        RichText::new("igralabs.com")
                            .small()
                            .color(Color32::from_rgb(18, 94, 82)),
                    );
                });
            }
        });
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
        .stroke(Stroke::new(1.0, STROKE))
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong().color(INK));
            ui.add_space(8.0);
            add_contents(ui)
        })
        .inner
}

fn secret_card(ui: &mut Ui, secrets: &ExportSecretsResponse, summary: &Option<WalletSummary>) {
    Frame::none()
        .fill(Color32::from_rgb(42, 19, 26))
        .inner_margin(egui::Margin::same(14.0))
        .rounding(egui::Rounding::same(14.0))
        .stroke(Stroke::new(1.0, WARM_RED))
        .show(ui, |ui| {
            ui.label(RichText::new("Sensitive material on screen").strong().color(WARM_RED));
            wrapped_text(
                ui,
                "Treat the following as temporary. Mnemonics should be written down offline, and only kpub strings are safe to share with cosigners.",
                INK,
            );
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
    wrapped_text(
        ui,
        &format!(
            "{} transaction(s) in bundle{}",
            bundle.transaction_count,
            if bundle.fully_signed {
                ", ready to broadcast"
            } else {
                ""
            }
        ),
        INK,
    );
    ui.add_space(8.0);

    for transaction in &bundle.transactions {
        Frame::none()
            .fill(PANEL_SOFT)
            .inner_margin(egui::Margin::same(12.0))
            .rounding(egui::Rounding::same(14.0))
            .stroke(Stroke::new(1.0, STROKE))
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
                    wrapped_text(
                        ui,
                        &format!(
                            "Input {}: {} of {} signatures collected",
                            progress.input_index, progress.signed_by, progress.minimum_signatures
                        ),
                        INK,
                    );
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
        MessageTone::Info => (PANEL_SOFT, TEAL),
        MessageTone::Error => (Color32::from_rgb(49, 18, 27), WARM_RED),
    };
    Frame::none()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .rounding(egui::Rounding::same(12.0))
        .stroke(Stroke::new(1.0, text))
        .show(ui, |ui| {
            wrapped_text(ui, &banner.text, text);
        });
}

fn status_chip(ui: &mut Ui, label: impl AsRef<str>, color: Color32) {
    Frame::none()
        .fill(color.gamma_multiply(0.12))
        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
        .rounding(egui::Rounding::same(999.0))
        .stroke(Stroke::new(1.0, color))
        .show(ui, |ui| {
            ui.label(RichText::new(label.as_ref()).color(color).small().strong());
        });
}

fn metric_line(ui: &mut Ui, label: &str, value: String) {
    ui.label(RichText::new(label).color(TEXT_SOFT));
    wrapped_text(ui, &value, INK);
}

fn mono_value(ui: &mut Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).color(TEXT_SOFT));
    mono_line(ui, value);
}

fn mono_line(ui: &mut Ui, value: &str) {
    Frame::none()
        .fill(PANEL_ALT)
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .rounding(egui::Rounding::same(10.0))
        .stroke(Stroke::new(1.0, STROKE))
        .show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), 0.0],
                Label::new(RichText::new(value).monospace().color(INK)).wrap(),
            );
        });
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

fn supporting_text(ui: &mut Ui, text: &str) {
    wrapped_text(ui, text, TEXT_SOFT);
}

fn wrapped_text(ui: &mut Ui, text: &str, color: Color32) {
    ui.add_sized(
        [ui.available_width(), 0.0],
        Label::new(RichText::new(text).color(color)).wrap(),
    );
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
        "configured" => TEAL,
        "syncing" | "running" | "starting" => COPPER,
        _ => WARM_RED,
    }
}

fn flow_stage(bundle: Option<&TransactionBundle>) -> (&'static str, Color32) {
    match bundle {
        None => ("awaiting bundle", TEXT_SOFT),
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
