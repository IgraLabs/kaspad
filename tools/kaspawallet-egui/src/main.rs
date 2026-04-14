mod backend;
mod theme;

use backend::{
    AddressesResponse, BackendClient, BackendProcess, BalanceResponse, BootstrapCreateRequest,
    BootstrapCreateResponse, BroadcastRequest, BroadcastResponse, CreateUnsignedRequest,
    DaemonStatus, ExportSecretsRequest, ExportSecretsResponse, FeePolicyRequest,
    NewAddressResponse, ParseRequest, SessionConfigRequest, SignRequest, TransactionBundle,
    WalletSummary, WalletSummaryRequest,
};
use eframe::egui::{self, Color32, ComboBox, Frame, Label, RichText, Stroke, TextEdit, Ui};
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

const INK: Color32 = Color32::from_rgb(245, 247, 250);
const TEXT_SOFT: Color32 = Color32::from_rgb(156, 165, 177);
const COPPER: Color32 = Color32::from_rgb(132, 168, 255);
const TEAL: Color32 = Color32::from_rgb(179, 246, 103);
const SAND: Color32 = Color32::from_rgb(8, 10, 14);
const CREAM: Color32 = Color32::from_rgb(16, 20, 27);
const PANEL_ALT: Color32 = Color32::from_rgb(21, 26, 35);
const PANEL_SOFT: Color32 = Color32::from_rgb(27, 33, 43);
const STROKE: Color32 = Color32::from_rgb(42, 50, 63);
const WARM_RED: Color32 = Color32::from_rgb(255, 122, 122);
const OLIVE: Color32 = Color32::from_rgb(120, 214, 128);

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1040.0, 720.0])
            .with_title("Kaspa Multisig Wallet"),
        ..Default::default()
    };

    eframe::run_native(
        "Kaspa Multisig Wallet",
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SetupMode {
    OpenExisting,
    CreateNew,
    RecoverExisting,
}

impl SetupMode {
    fn label(self) -> &'static str {
        match self {
            Self::OpenExisting => "Open wallet",
            Self::CreateNew => "Create wallet",
            Self::RecoverExisting => "Recover wallet",
        }
    }

    fn helper(self) -> &'static str {
        match self {
            Self::OpenExisting => "Load an existing multisig wallet file and inspect its role.",
            Self::CreateNew => {
                "Create a new multisig cosigner file and collect remote public keys."
            }
            Self::RecoverExisting => {
                "Recover this cosigner from mnemonic phrases and rebuild the wallet file."
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspacePage {
    Setup,
    Connect,
    Receive,
    Send,
    Technical,
}

impl WorkspacePage {
    fn label(self) -> &'static str {
        match self {
            Self::Setup => "Get Started",
            Self::Connect => "Connect",
            Self::Receive => "Receive",
            Self::Send => "Send",
            Self::Technical => "Technical",
        }
    }
}

struct BannerMessage {
    tone: MessageTone,
    text: String,
}

struct BootstrapForm {
    setup_mode: SetupMode,
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
            setup_mode: SetupMode::CreateNew,
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
    active_page: WorkspacePage,
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
                        text: "The local wallet backend is running. Start by opening, creating, or recovering a multisig wallet.".to_owned(),
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
            active_page: WorkspacePage::Setup,
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

    fn wallet_loaded(&self) -> bool {
        self.summary.is_some()
    }

    fn engine_state(&self) -> &str {
        self.daemon_status
            .as_ref()
            .map(|status| status.state.as_str())
            .unwrap_or("stopped")
    }

    fn wallet_ready(&self) -> bool {
        self.engine_state() == "ready"
    }

    fn can_issue_receive(&self) -> bool {
        self.summary
            .as_ref()
            .map(|summary| !summary.is_multisig || summary.is_canonical_address_owner)
            .unwrap_or(true)
    }

    fn sync_state_label(&self) -> &'static str {
        match self.engine_state() {
            "ready" => "Synced",
            "configured" | "starting" | "syncing" | "running" => "Syncing",
            _ => "Not connected",
        }
    }

    fn primary_stage(&self) -> (&'static str, Color32) {
        if !self.wallet_loaded() {
            ("Get started", COPPER)
        } else if !self.wallet_ready() {
            ("Connect wallet", COPPER)
        } else if self.flow_bundle.is_some() {
            ("Finish payment", OLIVE)
        } else if self.can_issue_receive() {
            ("Receive or send", TEAL)
        } else {
            ("Ready to sign", TEAL)
        }
    }

    fn page_unlocked(&self, page: WorkspacePage) -> bool {
        match page {
            WorkspacePage::Setup => true,
            WorkspacePage::Connect => self.wallet_loaded(),
            WorkspacePage::Receive => self.wallet_ready(),
            WorkspacePage::Send => self.wallet_ready(),
            WorkspacePage::Technical => {
                self.flow_bundle.is_some()
                    || !self.inspector.transactions_hex.trim().is_empty()
                    || self.inspector_bundle.is_some()
            }
        }
    }

    fn recommended_page(&self) -> WorkspacePage {
        if !self.wallet_loaded() {
            WorkspacePage::Setup
        } else if !self.wallet_ready() {
            WorkspacePage::Connect
        } else if self.flow_bundle.is_some() {
            WorkspacePage::Send
        } else if self.can_issue_receive() && self.last_new_address.is_none() {
            WorkspacePage::Receive
        } else {
            WorkspacePage::Send
        }
    }

    fn sync_active_page(&mut self) {
        if !self.page_unlocked(self.active_page) {
            self.active_page = self.recommended_page();
        }
    }

    fn next_action(&self) -> &'static str {
        if !self.wallet_loaded() {
            return "Create, recover, or open a multisig wallet.";
        }
        if self.node_form.rpc_server.trim().is_empty() {
            return "Add a Kaspa node RPC to sync this wallet.";
        }
        if !matches!(
            self.engine_state(),
            "configured" | "starting" | "syncing" | "running" | "ready"
        ) {
            return "Connect the wallet to your node.";
        }
        if !self.wallet_ready() {
            return "Wait for the wallet engine to finish syncing.";
        }
        if self.flow_bundle.is_some() {
            return "Review the payment, collect signatures, and send when it is fully signed.";
        }
        if self.can_issue_receive() && self.addresses.is_empty() && self.last_new_address.is_none()
        {
            return "Generate the next receive address or prepare a payment.";
        }
        "Wallet is ready for everyday receive and spend actions."
    }

    fn expected_remote_key_count(&self) -> u32 {
        self.bootstrap
            .num_public_keys
            .saturating_sub(self.bootstrap.num_private_keys)
    }

    fn provided_remote_key_count(&self) -> usize {
        self.bootstrap.remote_public_keys_vec().len()
    }

    fn setup_primary_action_label(&self) -> &'static str {
        match self.bootstrap.setup_mode {
            SetupMode::OpenExisting => "Open wallet",
            SetupMode::CreateNew => "Create wallet",
            SetupMode::RecoverExisting => "Recover wallet",
        }
    }

    fn setup_can_submit(&self) -> bool {
        if self.bootstrap.keys_file.trim().is_empty() {
            return false;
        }

        match self.bootstrap.setup_mode {
            SetupMode::OpenExisting => true,
            SetupMode::CreateNew | SetupMode::RecoverExisting => {
                if self.bootstrap.password.trim().is_empty() {
                    return false;
                }
                if self.bootstrap.num_private_keys == 0
                    || self.bootstrap.num_public_keys == 0
                    || self.bootstrap.minimum_signatures == 0
                {
                    return false;
                }
                if self.bootstrap.num_private_keys > self.bootstrap.num_public_keys {
                    return false;
                }
                if self.bootstrap.minimum_signatures > self.bootstrap.num_public_keys {
                    return false;
                }
                if self.provided_remote_key_count() != self.expected_remote_key_count() as usize {
                    return false;
                }
                if matches!(self.bootstrap.setup_mode, SetupMode::RecoverExisting)
                    && self.bootstrap.import_mnemonics_vec().len()
                        != self.bootstrap.num_private_keys as usize
                {
                    return false;
                }
                true
            }
        }
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
            import_mnemonics: if matches!(self.bootstrap.setup_mode, SetupMode::RecoverExisting) {
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
                            self.active_page = WorkspacePage::Connect;
                            self.set_banner(MessageTone::Info, "Wallet loaded.");
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
                                "Wallet file created and fingerprint confirmed."
                            } else {
                                created.canonical_owner_warning.as_str()
                            };
                            self.active_page = WorkspacePage::Connect;
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
                            self.active_page = WorkspacePage::Setup;
                            self.set_banner(
                                MessageTone::Info,
                                "Sensitive recovery material is visible below. Clear it when finished.",
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
                            let was_ready = self.wallet_ready();
                            self.daemon_status = Some(status.clone());
                            if let Some(wallet) = status.wallet.clone() {
                                self.summary = Some(wallet);
                            }
                            if status.state == "ready" && !was_ready {
                                self.active_page = if self.can_issue_receive() {
                                    WorkspacePage::Receive
                                } else {
                                    WorkspacePage::Send
                                };
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
                            self.active_page = WorkspacePage::Receive;
                            self.set_banner(
                                MessageTone::Info,
                                "Generated the next receive address.",
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
                            self.active_page = WorkspacePage::Send;
                            self.set_banner(MessageTone::Info, "Spend bundle updated.");
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
                            self.active_page = WorkspacePage::Technical;
                        }
                        Err(err) => self.set_banner(MessageTone::Error, err),
                    }
                }
                AppEvent::Broadcasted(result) => {
                    self.pending.remove("broadcast");
                    match result {
                        Ok(response) => {
                            self.last_broadcast_tx_ids = response.tx_ids.clone();
                            self.active_page = WorkspacePage::Send;
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
        let (stage_label, stage_color) = self.primary_stage();
        if ui.available_width() >= 980.0 {
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    ui.label(
                        RichText::new("Kaspa Multisig Wallet")
                            .text_style(egui::TextStyle::Name("Hero".into()))
                            .color(INK),
                    );
                    ui.add_space(2.0);
                    wrapped_text(ui, self.next_action(), TEXT_SOFT);
                });
                columns[1].vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        status_chip(ui, stage_label, stage_color);
                        status_chip(
                            ui,
                            self.sync_state_label(),
                            state_color(Some(self.engine_state())),
                        );
                    });
                });
            });
        } else {
            ui.label(
                RichText::new("Kaspa Multisig Wallet")
                    .text_style(egui::TextStyle::Name("Hero".into()))
                    .color(INK),
            );
            ui.add_space(2.0);
            wrapped_text(ui, self.next_action(), TEXT_SOFT);
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                status_chip(ui, stage_label, stage_color);
                status_chip(
                    ui,
                    self.sync_state_label(),
                    state_color(Some(self.engine_state())),
                );
            });
        }
    }

    fn render_sidebar(&mut self, ui: &mut Ui) {
        let (stage_label, stage_color) = self.primary_stage();
        render_sidebar_brand(ui, self.logo_texture.as_ref());
        ui.add_space(14.0);
        ui.label(RichText::new("Workspace").small().strong().color(TEXT_SOFT));
        ui.add_space(8.0);
        let mut pages = vec![
            WorkspacePage::Setup,
            WorkspacePage::Connect,
            WorkspacePage::Receive,
            WorkspacePage::Send,
        ];
        if self.page_unlocked(WorkspacePage::Technical)
            || self.active_page == WorkspacePage::Technical
        {
            pages.push(WorkspacePage::Technical);
        }

        for page in pages {
            if workspace_nav_item(
                ui,
                page,
                self.active_page == page,
                self.page_unlocked(page),
                page == self.recommended_page(),
            ) {
                self.active_page = page;
            }
            ui.add_space(2.0);
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(12.0);
        ui.label(RichText::new("Status").small().strong().color(TEXT_SOFT));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            status_chip(ui, stage_label, stage_color);
            status_chip(
                ui,
                self.sync_state_label(),
                state_color(Some(self.engine_state())),
            );
        });
        ui.add_space(8.0);
        ui.label(RichText::new(self.active_page.label()).strong().color(INK));
        supporting_text(ui, self.next_action());
    }

    fn render_bootstrap_editor(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Choose how you want to start").strong());
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.bootstrap.setup_mode,
                SetupMode::OpenExisting,
                SetupMode::OpenExisting.label(),
            );
            ui.selectable_value(
                &mut self.bootstrap.setup_mode,
                SetupMode::CreateNew,
                SetupMode::CreateNew.label(),
            );
            ui.selectable_value(
                &mut self.bootstrap.setup_mode,
                SetupMode::RecoverExisting,
                SetupMode::RecoverExisting.label(),
            );
        });
        supporting_text(ui, self.bootstrap.setup_mode.helper());
        self.bootstrap.import_mode =
            matches!(self.bootstrap.setup_mode, SetupMode::RecoverExisting);

        let previous_network = self.bootstrap.network;
        ui.add_space(12.0);
        ui.label(RichText::new("Wallet details").strong());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Network").small().strong().color(TEXT_SOFT));
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
            if ui.add(secondary_button("Use defaults")).clicked() {
                self.bootstrap.sync_defaults();
                self.node_form.sync_defaults(self.bootstrap.network);
            }
        });
        field(ui, "Wallet file", &mut self.bootstrap.keys_file);

        match self.bootstrap.setup_mode {
            SetupMode::OpenExisting => {
                supporting_text(
                    ui,
                    "Use this path when the signer file already exists and you only need to load it into the app.",
                );
            }
            SetupMode::CreateNew => {
                ui.add_space(12.0);
                ui.label(RichText::new("Create signer").strong());
                supporting_text(
                    ui,
                    "Set a password first, then define the multisig policy and add the public keys from the other cosigners.",
                );
                password_field(ui, "Wallet password", &mut self.bootstrap.password);

                ui.add_space(12.0);
                ui.label(RichText::new("Multisig policy").strong());
                supporting_text(
                    ui,
                    "Choose how many cosigners exist and how many signatures are required to approve a payment.",
                );
                ui.horizontal_wrapped(|ui| {
                    numeric_drag(
                        ui,
                        "Required signatures",
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

                ui.add_space(12.0);
                ui.label(RichText::new("Other cosigners").strong());
                supporting_text(
                    ui,
                    &format!(
                        "Paste the public keys from the other cosigners here. {} key(s) expected for this signer file.",
                        self.expected_remote_key_count()
                    ),
                );
                ui.add(
                    TextEdit::multiline(&mut self.bootstrap.remote_public_keys)
                        .desired_rows(6)
                        .hint_text("kpub... one per line"),
                );

                ui.add_space(10.0);
                egui::CollapsingHeader::new("Advanced wallet settings")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.checkbox(&mut self.bootstrap.ecdsa, "Use ECDSA wallet");
                        ui.checkbox(
                            &mut self.bootstrap.overwrite,
                            "Replace an existing wallet file",
                        );
                    });
            }
            SetupMode::RecoverExisting => {
                ui.add_space(12.0);
                ui.label(RichText::new("Recover signer").strong());
                supporting_text(
                    ui,
                    "Rebuild this signer file from recovery words, then add the public keys from the other cosigners.",
                );
                password_field(ui, "Wallet password", &mut self.bootstrap.password);

                ui.add_space(12.0);
                ui.label(RichText::new("Multisig policy").strong());
                ui.horizontal_wrapped(|ui| {
                    numeric_drag(
                        ui,
                        "Required signatures",
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

                ui.add_space(12.0);
                ui.label(RichText::new("Other cosigners").strong());
                supporting_text(
                    ui,
                    &format!(
                        "Paste the public keys from the other cosigners here. {} key(s) expected for this signer file.",
                        self.expected_remote_key_count()
                    ),
                );
                ui.add(
                    TextEdit::multiline(&mut self.bootstrap.remote_public_keys)
                        .desired_rows(6)
                        .hint_text("kpub... one per line"),
                );

                ui.add_space(12.0);
                ui.label(RichText::new("Recovery words").strong());
                supporting_text(
                    ui,
                    "Enter one mnemonic phrase per local key. The count must match the number of local keys in this signer file.",
                );
                ui.add(
                    TextEdit::multiline(&mut self.bootstrap.import_mnemonics)
                        .desired_rows(4)
                        .hint_text("word1 word2 ... word24"),
                );

                ui.add_space(10.0);
                egui::CollapsingHeader::new("Advanced wallet settings")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.checkbox(&mut self.bootstrap.ecdsa, "Use ECDSA wallet");
                        ui.checkbox(
                            &mut self.bootstrap.overwrite,
                            "Replace an existing wallet file",
                        );
                    });
            }
        }
    }

    fn render_bootstrap_summary_panel(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new("Wallet details")
                .text_style(egui::TextStyle::Name("Section".into()))
                .color(INK),
        );
        ui.add_space(6.0);
        supporting_text(
            ui,
            "Once the wallet file is ready, you can connect it to a node and continue with receive or send actions.",
        );
        ui.add_space(12.0);

        if let Some(summary) = &self.summary {
            metric_line(ui, "Fingerprint", summary.fingerprint.clone());
            metric_line(ui, "Your signer slot", summary.cosigner_index.to_string());
            mono_value(ui, "Wallet file", &summary.keys_file);
            ui.add_space(10.0);
            if self.can_issue_receive() {
                highlight_strip(
                    ui,
                    "Wallet role",
                    "This signer file can generate receive addresses and can also sign payments.",
                    TEAL,
                );
            } else {
                highlight_strip(
                    ui,
                    "Wallet role",
                    "This signer file can sign payments but cannot generate receive addresses. Use the primary signer to receive funds.",
                    WARM_RED,
                );
            }
        } else {
            supporting_text(
                ui,
                "Complete this step to load the signer fingerprint and receive permissions for this wallet.",
            );
        }

        ui.add_space(14.0);
        ui.horizontal_wrapped(|ui| {
            if matches!(self.bootstrap.setup_mode, SetupMode::OpenExisting) {
                if ui
                    .add(primary_button(self.setup_primary_action_label()))
                    .clicked()
                {
                    self.request_wallet_summary();
                }
            } else {
                ui.add_enabled_ui(self.setup_can_submit(), |ui| {
                    if ui
                        .add(primary_button(self.setup_primary_action_label()))
                        .clicked()
                    {
                        self.request_wallet_create();
                    }
                });
            }
            if ui
                .add_enabled(
                    self.wallet_loaded() && !self.bootstrap.password.trim().is_empty(),
                    secondary_button("Show recovery material"),
                )
                .clicked()
            {
                self.request_export_secrets();
            }
        });
        supporting_text(
            ui,
            "Recovery words are sensitive. Reveal them only when you are ready to store or verify them offline.",
        );
    }

    fn render_connect_section(&mut self, ui: &mut Ui) {
        section_card(
            ui,
            "Connect Wallet",
            "Attach this signer file to a Kaspa node. The app manages the wallet engine for you in the background.",
            COPPER,
            |ui| {
                field(ui, "Node endpoint", &mut self.node_form.rpc_server);
                supporting_text(
                    ui,
                    "You only need to provide the node endpoint. Starting, restarting, and health checks happen automatically.",
                );
                ui.add_space(10.0);
                ui.add_enabled_ui(self.wallet_loaded(), |ui| {
                    if ui.add(primary_button("Connect and sync")).clicked() {
                        self.request_connect_node();
                    }
                });

                if !self.wallet_loaded() {
                    supporting_text(
                        ui,
                        "Complete wallet setup first, then return here to connect and sync.",
                    );
                }

                ui.add_space(14.0);
                self.render_receive_status_panel(ui);
            },
        );
    }

    fn render_receive_controls(&mut self, ui: &mut Ui) {
        if !self.wallet_ready() {
            supporting_text(
                ui,
                "Wait until the wallet is synced before requesting the next receive address.",
            );
            return;
        }

        ui.label(RichText::new("Receive funds").strong());
        supporting_text(
            ui,
            "Generate the next receive address only from the cosigner that owns receive addresses for this wallet.",
        );
        ui.add_space(8.0);
        ui.add_enabled_ui(self.can_issue_receive(), |ui| {
            if ui.add(primary_button("Generate receive address")).clicked() {
                self.request_new_address();
            }
        });
        if !self.can_issue_receive() {
            wrapped_text(
                ui,
                "This signer can approve payments but cannot create receive addresses. Use the primary receive-capable cosigner instead.",
                WARM_RED,
            );
        }

        if let Some(address) = &self.last_new_address {
            ui.add_space(12.0);
            mono_value(ui, "Current receive address", address);
        }
    }

    fn render_receive_status_panel(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Connection details").strong());
        ui.label(
            RichText::new(self.sync_state_label())
                .color(state_color(Some(self.engine_state())))
                .strong(),
        );
        if let Some(status) = &self.daemon_status {
            wrapped_text(ui, &status.message, INK);
            if let Some(started_at) = &status.started_at {
                metric_line(ui, "Started", started_at.clone());
            }
            if !status.rpc_server.is_empty() {
                mono_value(ui, "Node endpoint", &status.rpc_server);
            }
            if let Some(version) = &status.wallet_version {
                metric_line(ui, "Wallet engine", version.clone());
            }
        } else {
            supporting_text(ui, "Provide a node RPC to start syncing this wallet.");
        }

        ui.add_space(10.0);
        if ui.add(secondary_button("Refresh sync")).clicked() {
            self.request_daemon_status();
        }
    }

    fn render_spend_controls(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Prepare payment").strong());
        supporting_text(
            ui,
            "Start with the destination and amount. Extra routing and fee controls stay hidden unless you need them.",
        );
        ui.add_space(8.0);
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

        ui.add_space(10.0);
        ui.add_enabled_ui(self.wallet_ready(), |ui| {
            if ui.add(primary_button("Prepare payment")).clicked() {
                self.request_create_unsigned();
            }
        });

        if !self.wallet_ready() {
            supporting_text(
                ui,
                "The wallet must finish syncing before you can prepare a payment.",
            );
        }

        ui.add_space(10.0);
        egui::CollapsingHeader::new("Advanced payment options")
            .default_open(false)
            .show(ui, |ui| {
                ui.checkbox(
                    &mut self.spend.use_existing_change_address,
                    "Prefer an existing change address",
                );
                ui.label(RichText::new("From addresses (optional)").strong());
                supporting_text(
                    ui,
                    "Restrict UTXOs to specific wallet addresses. Leave blank to spend from the full wallet.",
                );
                ui.add(
                    TextEdit::multiline(&mut self.spend.from_addresses)
                        .desired_rows(3)
                        .hint_text("one address per line"),
                );

                ui.add_space(10.0);
                if ui.available_width() < 520.0 {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Fee policy")
                                .small()
                                .strong()
                                .color(TEXT_SOFT),
                        );
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
                        ui.label(
                            RichText::new("Fee policy")
                                .small()
                                .strong()
                                .color(TEXT_SOFT),
                        );
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
            });
    }

    fn render_spend_status_panel(&mut self, ui: &mut Ui) {
        let stage = flow_stage(self.flow_bundle.as_ref());
        ui.label(RichText::new("Review and finish").strong());
        ui.add_space(6.0);
        status_chip(ui, stage.0, stage.1);
        if let Some(bundle) = &self.flow_bundle {
            let fully_signed = bundle.fully_signed;
            let missing = bundle
                .transactions
                .iter()
                .flat_map(|tx| tx.signatures.iter())
                .map(|progress| progress.missing_signatures)
                .max()
                .unwrap_or(0);
            metric_line(ui, "Transactions", bundle.transaction_count.to_string());
            metric_line(ui, "Missing signatures", missing.to_string());
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.flow_hex.trim().is_empty()
                            && !self.bootstrap.password.trim().is_empty(),
                        secondary_button("Sign locally"),
                    )
                    .clicked()
                {
                    self.request_sign_flow();
                }
                if ui
                    .add_enabled(
                        !self.flow_hex.trim().is_empty(),
                        secondary_button("Review transaction"),
                    )
                    .clicked()
                {
                    self.request_parse_flow();
                }
                if ui
                    .add_enabled(fully_signed, primary_button("Send now"))
                    .clicked()
                {
                    self.request_broadcast_flow();
                }
            });
        } else {
            supporting_text(
                ui,
                "Prepared payments appear here after you create the first unsigned bundle.",
            );
        }
    }

    fn render_bootstrap_section(&mut self, ui: &mut Ui) {
        section_card(ui, "1. Get Started", "Choose one wallet action first. The rest of the app stays quiet until this signer file is ready.", COPPER, |ui| {
            self.render_bootstrap_editor(ui);
            ui.add_space(16.0);
            self.render_bootstrap_summary_panel(ui);

            if let Some(secrets) = self.secrets.clone() {
                ui.add_space(14.0);
                egui::CollapsingHeader::new("Reveal recovery words")
                    .default_open(false)
                    .show(ui, |ui| {
                        secret_card(ui, &secrets, &self.summary);
                        ui.add_space(8.0);
                        if ui.add(danger_button("Clear recovery material")).clicked() {
                            self.secrets = None;
                        }
                    });
            }
        });
    }

    fn render_receive_section(&mut self, ui: &mut Ui) {
        section_card(
            ui,
            "Receive Funds",
            "This page only handles receive addresses. Connection and sync status live in the separate Connect step.",
            TEAL,
            |ui| {
                self.render_receive_controls(ui);

                if let Some(balance) = &self.balance {
                    ui.add_space(12.0);
                    metric_line(ui, "Available", format!("{} KAS", balance.available_kas));
                    metric_line(ui, "Pending", format!("{} KAS", balance.pending_kas));
                }

                if self.wallet_ready() {
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(secondary_button("Refresh balances")).clicked() {
                            self.request_balance();
                        }
                        if ui.add(secondary_button("Load address history")).clicked() {
                            self.request_addresses();
                        }
                    });
                }

                if !self.addresses.is_empty() {
                    ui.add_space(10.0);
                    egui::CollapsingHeader::new("Address history")
                        .default_open(false)
                        .show(ui, |ui| {
                            for address in self.addresses.iter().take(12) {
                                mono_line(ui, address);
                            }
                        });
                }
            },
        );
    }

    fn render_spend_section(&mut self, ui: &mut Ui) {
        section_card(
            ui,
            "Send Funds",
            "Prepare the payment first. Review, signatures, and final send actions only appear after a real transaction draft exists.",
            OLIVE,
            |ui| {
                if self.flow_bundle.is_some() {
                    supporting_text(
                        ui,
                        "A payment draft already exists. Finish review and signatures here, or clear it to start a different payment.",
                    );
                    ui.add_space(16.0);
                    self.render_spend_status_panel(ui);
                    ui.add_space(12.0);
                    if ui.add(secondary_button("Start a new payment")).clicked() {
                        self.flow_hex.clear();
                        self.flow_bundle = None;
                        self.last_broadcast_tx_ids.clear();
                    }
                    ui.add_space(12.0);
                    supporting_text(
                        ui,
                        "Need raw bundle hex or a deeper inspection? Open the Technical page from the status rail.",
                    );
                    if ui
                        .add(secondary_button("Open technical details"))
                        .clicked()
                    {
                        self.active_page = WorkspacePage::Technical;
                    }
                } else {
                    self.render_spend_controls(ui);
                }

                if !self.last_broadcast_tx_ids.is_empty() {
                    ui.add_space(14.0);
                    ui.label(RichText::new("Last broadcast txids").strong());
                    for txid in &self.last_broadcast_tx_ids {
                        mono_line(ui, txid);
                    }
                }
            },
        );
    }

    fn render_inspector_section(&mut self, ui: &mut Ui) {
        section_card(
            ui,
            "Technical Details",
            "Use this page for raw bundle hex, offline parsing, and deeper transaction inspection outside the normal receive and send flow.",
            COPPER,
            |ui| {
                if !self.flow_hex.trim().is_empty() {
                    ui.label(RichText::new("Current payment bundle").strong());
                    ui.add(
                        TextEdit::multiline(&mut self.flow_hex)
                            .desired_rows(8)
                            .font(egui::TextStyle::Monospace)
                            .hint_text("Unsigned, partially signed, or fully signed bundle hex."),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(secondary_button("Copy bundle hex")).clicked() {
                            copy_text(ui, self.flow_hex.clone());
                        }
                        if ui.add(secondary_button("Use in inspector")).clicked() {
                            self.inspector.transactions_hex = self.flow_hex.clone();
                        }
                        if ui.add(danger_button("Clear current bundle")).clicked() {
                            self.flow_hex.clear();
                            self.flow_bundle = None;
                            self.last_broadcast_tx_ids.clear();
                        }
                    });

                    if let Some(bundle) = &self.flow_bundle {
                        ui.add_space(10.0);
                        egui::CollapsingHeader::new("Bundle breakdown")
                            .default_open(false)
                            .show(ui, |ui| {
                                render_transaction_bundle(ui, bundle);
                            });
                    }

                    ui.add_space(16.0);
                }

                ui.label(RichText::new("Inspector").strong());
                ui.add(
                    TextEdit::multiline(&mut self.inspector.transactions_hex)
                        .desired_rows(8)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("Paste any bundle hex here for offline review."),
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.add(primary_button("Parse inspector hex")).clicked() {
                        self.request_parse_inspector();
                    }
                    if ui.add(secondary_button("Use current bundle")).clicked() {
                        self.inspector.transactions_hex = self.flow_hex.clone();
                    }
                    if ui.add(secondary_button("Copy inspector hex")).clicked() {
                        copy_text(ui, self.inspector.transactions_hex.clone());
                    }
                });

                if let Some(bundle) = &self.inspector_bundle {
                    ui.add_space(14.0);
                    render_transaction_bundle(ui, bundle);
                }
            },
        );
    }
}

impl eframe::App for WalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        self.auto_poll();
        self.sync_active_page();
        ctx.request_repaint_after(Duration::from_millis(250));

        egui::TopBottomPanel::top("top_bar")
            .frame(
                Frame::none()
                    .fill(SAND)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                self.render_top_bar(ui);
                if let Some(banner) = &self.banner {
                    ui.add_space(8.0);
                    banner_line(ui, banner);
                }
            });

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(268.0)
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
                egui::ScrollArea::vertical().show(ui, |ui| match self.active_page {
                    WorkspacePage::Setup => self.render_bootstrap_section(ui),
                    WorkspacePage::Connect => self.render_connect_section(ui),
                    WorkspacePage::Receive => self.render_receive_section(ui),
                    WorkspacePage::Send => self.render_spend_section(ui),
                    WorkspacePage::Technical => self.render_inspector_section(ui),
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
        .inner_margin(egui::Margin::same(16.0))
        .rounding(egui::Rounding::same(16.0))
        .stroke(Stroke::new(1.0, STROKE.gamma_multiply(0.6)))
        .show(ui, |ui| {
            ui.label(
                RichText::new(title.to_ascii_uppercase())
                    .small()
                    .strong()
                    .color(accent),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(title)
                    .text_style(egui::TextStyle::Name("Section".into()))
                    .color(INK),
            );
            ui.add_space(2.0);
            supporting_text(ui, subtitle);
            ui.add_space(12.0);
            add_contents(ui)
        })
        .inner
}

fn render_sidebar_brand(ui: &mut Ui, logo_texture: Option<&egui::TextureHandle>) {
    if let Some(texture) = logo_texture {
        ui.add(
            egui::Image::new(texture)
                .fit_to_exact_size(egui::vec2(138.0, 42.0))
                .maintain_aspect_ratio(true),
        );
    } else {
        ui.label(RichText::new("IGRA LABS").strong().color(INK));
    }
}

fn highlight_strip(ui: &mut Ui, label: &str, value: &str, accent: Color32) {
    Frame::none()
        .fill(PANEL_SOFT)
        .inner_margin(egui::Margin::same(12.0))
        .rounding(egui::Rounding::same(12.0))
        .stroke(Stroke::new(1.0, STROKE.gamma_multiply(0.45)))
        .show(ui, |ui| {
            ui.label(
                RichText::new(label.to_ascii_uppercase())
                    .small()
                    .strong()
                    .color(accent),
            );
            ui.add_space(4.0);
            wrapped_text(ui, value, INK);
        });
}

fn workspace_nav_item(
    ui: &mut Ui,
    page: WorkspacePage,
    active: bool,
    unlocked: bool,
    recommended: bool,
) -> bool {
    let fill = if active {
        PANEL_SOFT.gamma_multiply(0.7)
    } else {
        Color32::TRANSPARENT
    };
    let title_color = if unlocked { INK } else { TEXT_SOFT };

    let response = Frame::none()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .rounding(egui::Rounding::same(10.0))
        .show(ui, |ui| {
            ui.add_enabled_ui(unlocked, |ui| {
                ui.horizontal(|ui| {
                    let marker = if active {
                        COPPER
                    } else if recommended {
                        TEAL.gamma_multiply(0.75)
                    } else {
                        STROKE.gamma_multiply(0.45)
                    };
                    ui.colored_label(marker, "●");
                    ui.label(RichText::new(page.label()).strong().color(title_color));
                });
            });
        })
        .response
        .interact(egui::Sense::click());

    unlocked && response.clicked()
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
        MessageTone::Info => (PANEL_ALT, TEAL),
        MessageTone::Error => (Color32::from_rgb(41, 18, 23), WARM_RED),
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
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .rounding(egui::Rounding::same(999.0))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.75)))
        .show(ui, |ui| {
            ui.label(RichText::new(label.as_ref()).color(color).small().strong());
        });
}

fn primary_button(label: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.into()).strong().color(SAND))
        .fill(TEAL)
        .stroke(Stroke::new(1.0, TEAL))
        .rounding(egui::Rounding::same(12.0))
}

fn secondary_button(label: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.into()).strong().color(INK))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .rounding(egui::Rounding::same(12.0))
}

fn danger_button(label: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.into()).strong().color(WARM_RED))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, WARM_RED))
        .rounding(egui::Rounding::same(12.0))
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
        .show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), 0.0],
                Label::new(RichText::new(value).monospace().color(INK)).wrap(),
            );
        });
}

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).small().strong().color(TEXT_SOFT));
    ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn password_field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).small().strong().color(TEXT_SOFT));
    ui.add(
        TextEdit::singleline(value)
            .password(true)
            .desired_width(f32::INFINITY),
    );
}

fn numeric_drag(ui: &mut Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).small().strong().color(TEXT_SOFT));
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
