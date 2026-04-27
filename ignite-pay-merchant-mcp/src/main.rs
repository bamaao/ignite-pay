use ignite_pay_merchant_mcp::audit::AuditLogStore;
use ignite_pay_merchant_mcp::channel::MerchantChannelClient;
use ignite_pay_merchant_mcp::config;
use ignite_pay_merchant_mcp::mediator::MerchantMediator;
use ignite_pay_merchant_mcp::payment::{PaymentOrder, PaymentOrderStore};
use ignite_pay_merchant_mcp::qr::{self, PaymentQrData};
use ignite_pay_merchant_mcp::tools::{
    CheckPaymentInput, CloseChannelInput, GeneratePaymentQrInput, GetChannelStatusInput,
    GetPaymentHistoryInput, OpenChannelWithHubInput, SettleChannelInput,
};

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::ServerCapabilities,
    tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler, ServiceExt,
};
use std::sync::Arc;
use tracing_subscriber::fmt::writer::MakeWriterExt;

// ── MCP Server ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct MerchantMcpServer {
    tool_router: ToolRouter<Self>,
    mediator: Arc<MerchantMediator>,
    orders: Arc<PaymentOrderStore>,
    channel_client: Option<Arc<MerchantChannelClient>>,
    audit: Arc<AuditLogStore>,
    hub_endpoint: String,
}

#[tool_router]
impl MerchantMcpServer {
    #[tool(description = "Generate a payment QR code for receiving state channel payments. Returns QR text and ASCII representation.")]
    async fn generate_payment_qr(
        &self,
        Parameters(input): Parameters<GeneratePaymentQrInput>,
    ) -> String {
        let order_id = input
            .order_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let qr_data = PaymentQrData {
            qr_type: "ignite-pay-request".to_string(),
            version: 1,
            merchant_did: self.mediator.our_did().to_string(),
            amount: input.amount,
            description: input.description.clone(),
            order_id: order_id.clone(),
            hub_endpoint: self.hub_endpoint.clone(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        let order = PaymentOrder {
            order_id: order_id.clone(),
            merchant_did: self.mediator.our_did().to_string(),
            amount: input.amount,
            description: input.description.clone(),
            hub_endpoint: self.hub_endpoint.clone(),
            status: ignite_pay_merchant_mcp::payment::OrderStatus::Pending,
            created_at: chrono::Utc::now(),
            confirmed_at: None,
            channel_id: None,
            leaf_index: None,
            sequence: None,
        };
        if let Err(e) = self.orders.save_order(&order) {
            return format!("Error saving order: {}", e);
        }

        let _ = self.audit.append("qr_generated", Some(&order_id), Some(input.amount), "Payment QR generated");

        let qr_text = qr::generate_payment_qr_text(&qr_data);
        let ascii = match qr::generate_qr_ascii(&qr_data) {
            Ok(a) => a,
            Err(e) => format!("(QR render error: {})", e),
        };

        format!(
            "Payment QR generated.\nOrder: {}\nAmount: {}\nDescription: {}\n\nQR Text: {}\n\nASCII:\n{}",
            order_id, input.amount, input.description, qr_text, ascii
        )
    }

    #[tool(description = "Check the status of a payment order by order_id.")]
    async fn check_payment(&self, Parameters(input): Parameters<CheckPaymentInput>) -> String {
        match self.orders.get_order(&input.order_id) {
            Ok(Some(order)) => {
                let confirmed = order
                    .confirmed_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "N/A".to_string());
                format!(
                    "Order: {}\nMerchant: {}\nAmount: {}\nDescription: {}\nStatus: {}\nCreated: {}\nConfirmed: {}\nChannel: {}\nLeaf: {}",
                    order.order_id,
                    order.merchant_did,
                    order.amount,
                    order.description,
                    order.status,
                    order.created_at.to_rfc3339(),
                    confirmed,
                    order.channel_id.as_deref().unwrap_or("N/A"),
                    order.leaf_index.map(|i| i.to_string()).as_deref().unwrap_or("N/A"),
                )
            }
            Ok(None) => format!("Order {} not found.", input.order_id),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Get recent payment history.")]
    async fn get_payment_history(
        &self,
        Parameters(input): Parameters<GetPaymentHistoryInput>,
    ) -> String {
        match self.orders.list_orders(input.limit) {
            Ok(orders) => {
                if orders.is_empty() {
                    return "No payment orders found.".to_string();
                }
                let mut result = format!("Payment History ({} orders):\n\n", orders.len());
                for order in &orders {
                    result.push_str(&format!(
                        "- {} | {} | {} | {} | {}\n",
                        &order.order_id[..8.min(order.order_id.len())],
                        order.status,
                        order.amount,
                        order.description.chars().take(20).collect::<String>(),
                        order.created_at.format("%Y-%m-%d %H:%M"),
                    ));
                }
                result
            }
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Get state channel status. If channel_id is provided, shows details. Otherwise lists all channels.")]
    async fn get_channel_status(
        &self,
        Parameters(input): Parameters<GetChannelStatusInput>,
    ) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "State channel client not configured.".to_string(),
        };

        match &input.channel_id {
            Some(id) => match client.get_channel_status(id) {
                Ok(s) => format!(
                    "Channel: {}\nStatus: {}\nSequence: {}\nLeaves: {}\nProvider Balance: {}\nTotal Deposited: {}",
                    s.channel_id, s.status, s.sequence, s.leaf_count, s.provider_balance, s.total_deposited
                ),
                Err(e) => format!("Error: {}", e),
            },
            None => match client.list_channels() {
                Ok(channels) => {
                    if channels.is_empty() {
                        return "No channels found.".to_string();
                    }
                    let mut result = format!("Channels ({}):\n\n", channels.len());
                    for id in &channels {
                        match client.get_channel_status(id) {
                            Ok(s) => result.push_str(&format!(
                                "- {} | {} | seq={} | balance={}\n",
                                &id[..16.min(id.len())], s.status, s.sequence, s.provider_balance
                            )),
                            Err(_) => result.push_str(&format!("- {} | (error loading)\n", &id[..16.min(id.len())])),
                        }
                    }
                    result
                }
                Err(e) => format!("Error: {}", e),
            },
        }
    }

    #[tool(description = "Open a state channel with a Hub as the Provider (merchant). This allows receiving payments.")]
    async fn open_channel_with_hub(
        &self,
        Parameters(_input): Parameters<OpenChannelWithHubInput>,
    ) -> String {
        "As a provider (merchant), channels are opened by users. Use get_identity to share your provider pubkey with users.".to_string()
    }

    #[tool(description = "Cooperatively close a state channel.")]
    async fn close_channel(&self, Parameters(input): Parameters<CloseChannelInput>) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "State channel client not configured.".to_string(),
        };

        match client.close_channel(&input.channel_id).await {
            Ok(msg) => {
                let _ = self.audit.append("channel_closed", Some(&input.channel_id), None, "Channel cooperatively closed");
                msg
            }
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Settle a state channel: claim leaves and finalize on-chain.")]
    async fn settle_channel(&self, Parameters(input): Parameters<SettleChannelInput>) -> String {
        let client = match &self.channel_client {
            Some(c) => c,
            None => return "State channel client not configured.".to_string(),
        };

        let claim_result = client.claim_leaf(&input.channel_id, 0, 0).await;
        let claim_msg = match claim_result {
            Ok(msg) => msg,
            Err(e) => format!("Claim error: {}", e),
        };

        let finalize_result = client.finalize(&input.channel_id).await;
        let finalize_msg = match finalize_result {
            Ok(msg) => {
                let _ = self.audit.append("channel_settled", Some(&input.channel_id), None, "Channel settled and finalized");
                msg
            }
            Err(e) => format!("Finalize error: {}", e),
        };

        format!("{}\n{}", claim_msg, finalize_msg)
    }

    #[tool(description = "Get merchant identity: DID, Hub connection status, and mediator connection info.")]
    async fn get_identity(&self, Parameters(_): Parameters<()>) -> String {
        let did = self.mediator.our_did();
        let channel_status = if self.channel_client.is_some() {
            "configured"
        } else {
            "not configured"
        };

        format!(
            "Merchant DID: {}\nHub Endpoint: {}\nChannel Client: {}",
            did, self.hub_endpoint, channel_status
        )
    }
}

#[tool_handler]
impl ServerHandler for MerchantMcpServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_instructions(
            "Ignite Pay Merchant MCP — generate payment QR codes, receive state channel payments, manage orders. \
             Use generate_payment_qr to create a payment QR code, check_payment to verify payment status.",
        )
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if let Ok(log_dir) = std::env::var("AUDIT_LOG_DIR") {
        let file_appender = tracing_appender::rolling::daily(&log_dir, "merchant-mcp.log");
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr.and(file_appender))
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(env_filter)
            .init();
    }

    let config = config::load_config()?;
    tracing::info!("Loaded merchant config: hub={}", config.merchant.hub_endpoint);

    let db = sled::open(&config.storage.path)?;
    tracing::info!("Database opened at {}", config.storage.path);

    let mediator = Arc::new(MerchantMediator::new(&config.mediator.ws_url, &db)?);

    // Create channel for create-channel-request commands from DIDComm messages
    let (create_channel_tx, mut create_channel_rx) =
        tokio::sync::mpsc::unbounded_channel::<ignite_pay_merchant_mcp::mediator::CreateChannelCommand>();
    mediator.connect(Some(create_channel_tx)).await?;
    tracing::info!("Merchant mediator connected");

    // Auto-display pairing QR if no merchant app is paired
    if mediator.paired_phone_did().await.is_none() {
        let url = mediator.generate_invitation();
        match mediator.generate_invitation_qr() {
            Ok(qr) => {
                tracing::info!("\n{}", qr);
                tracing::info!("\nInvitation URL:\n{}", url);
                tracing::info!("Scan the QR code above with Ignite Pay Merchant to pair.");
            }
            Err(e) => {
                tracing::warn!("Failed to generate QR code: {}. Invitation URL:\n{}", e, url);
                tracing::info!("Use the invitation URL above to pair manually.");
            }
        }
    } else if let Some(phone) = mediator.paired_phone_did().await {
        tracing::info!("Merchant app already paired: {}", phone);
    }

    let orders = Arc::new(PaymentOrderStore::from_db(db.clone()));
    let audit = Arc::new(AuditLogStore::from_db(db.clone()));

    // Initialize channel client
    let channel_client = if !config.merchant.hub_endpoint.is_empty() {
        let channel_db = sled::open(format!("{}/channel", config.storage.path))?;
        let keypair_bytes = MerchantChannelClient::generate_keypair();
        match MerchantChannelClient::new(&config.merchant.hub_endpoint, channel_db, &keypair_bytes)
        {
            Ok(client) => {
                tracing::info!(
                    "Channel client initialized: hub={}, pubkey={}",
                    config.merchant.hub_endpoint,
                    client.pubkey()
                );
                Some(Arc::new(client))
            }
            Err(e) => {
                tracing::error!("Failed to initialize channel client: {}", e);
                None
            }
        }
    } else {
        None
    };

    let hub_endpoint = config.merchant.hub_endpoint.clone();

    // Spawn task to handle create-channel commands from DIDComm messages
    {
        let mediator_clone = mediator.clone();
        let channel_client_clone = channel_client.clone();
        tokio::spawn(async move {
            while let Some(cmd) = create_channel_rx.recv().await {
                tracing::info!(
                    "Processing create-channel command from {}: hub={}",
                    cmd.requestor_did,
                    cmd.hub_endpoint
                );

                let result = if let Some(ref client) = channel_client_clone {
                    client
                        .open_channel(
                            &cmd.provider_pubkey,
                            &cmd.token_mint,
                            cmd.deposit,
                            cmd.tree_depth,
                        )
                        .await
                } else {
                    Err(anyhow::anyhow!("Channel client not configured"))
                };

                match result {
                    Ok(open_result) => {
                        if let Err(e) = mediator_clone
                            .send_create_channel_response(
                                &cmd.requestor_did,
                                &open_result.channel_id,
                                open_result.sequence,
                                &open_result.current_root,
                                true,
                                None,
                            )
                            .await
                        {
                            tracing::error!("Failed to send create-channel response: {}", e);
                        }
                    }
                    Err(e) => {
                        if let Err(e2) = mediator_clone
                            .send_create_channel_response(
                                &cmd.requestor_did,
                                "",
                                0,
                                "",
                                false,
                                Some(&e.to_string()),
                            )
                            .await
                        {
                            tracing::error!("Failed to send create-channel error response: {}", e2);
                        }
                    }
                }
            }
        });
    }

    let server = MerchantMcpServer {
        tool_router: MerchantMcpServer::tool_router(),
        mediator,
        orders,
        channel_client,
        audit,
        hub_endpoint,
    };

    tracing::info!("Starting merchant MCP server on stdio...");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
