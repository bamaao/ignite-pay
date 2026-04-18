use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::config::Role;
use crate::state::AppState;

/// Build the axum router based on the service role.
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::permissive();

    let mut router = Router::new()
        .route("/health", get(crate::handlers::health::health))
        .route("/ws", get(crate::ws::session::ws_handler));

    match state.role {
        Role::User => {
            router = router
                // Channel lifecycle
                .route("/v1/channels/open", post(crate::handlers::channel::open_channel))
                .route("/v1/channels/{id}/fund", post(crate::handlers::channel::fund_channel))
                .route("/v1/channels", get(crate::handlers::channel::list_channels))
                .route("/v1/channels/{id}", get(crate::handlers::channel::get_channel))
                // Payments
                .route("/v1/channels/{id}/split", post(crate::handlers::payment::split))
                .route("/v1/channels/{id}/pay", post(crate::handlers::payment::pay))
                .route("/v1/channels/{id}/batch", post(crate::handlers::payment::batch_update))
                // Settlement
                .route("/v1/channels/{id}/cosign", post(crate::handlers::settlement::request_cosign))
                .route("/v1/channels/{id}/close", post(crate::handlers::settlement::cooperative_close))
                .route("/v1/channels/{id}/challenge", post(crate::handlers::settlement::trigger_challenge))
                .route("/v1/channels/{id}/settle", post(crate::handlers::settlement::settle))
                .route("/v1/channels/{id}/claim", post(crate::handlers::settlement::claim))
                .route("/v1/channels/{id}/finalize", post(crate::handlers::settlement::finalize))
                // HTLC
                .route("/v1/channels/{id}/htlc/create", post(crate::handlers::htlc::create_htlc))
                .route("/v1/channels/{id}/htlc/resolve", post(crate::handlers::htlc::resolve_htlc))
                .route("/v1/channels/{id}/htlc/refund", post(crate::handlers::htlc::refund_htlc))
                // Routing & multi-hop
                .route("/v1/routes", get(crate::handlers::routing::find_routes))
                .route("/v1/multihop/create", post(crate::handlers::multihop::create_payment))
                .route("/v1/multihop/{id}/resolve", post(crate::handlers::multihop::resolve_hop))
                // Compliance
                .route("/v1/compliance/{id}", get(crate::handlers::compliance::get_status));
        }
        Role::Provider => {
            router = router
                // Channel (provider subset)
                .route("/v1/channels/{id}/fund", post(crate::handlers::channel::fund_channel))
                .route("/v1/channels", get(crate::handlers::channel::list_channels))
                .route("/v1/channels/{id}", get(crate::handlers::channel::get_channel))
                // Payments (provider accept)
                .route("/v1/channels/{id}/cosign", post(crate::handlers::settlement::provider_cosign))
                .route("/v1/channels/{id}/accept-payment", post(crate::handlers::payment::accept_payment))
                .route("/v1/channels/{id}/accept-batch", post(crate::handlers::payment::accept_batch))
                // Settlement
                .route("/v1/channels/{id}/close", post(crate::handlers::settlement::cooperative_close))
                .route("/v1/channels/{id}/challenge", post(crate::handlers::settlement::trigger_challenge))
                .route("/v1/channels/{id}/submit-counter", post(crate::handlers::settlement::submit_counter))
                .route("/v1/channels/{id}/claim", post(crate::handlers::settlement::claim))
                .route("/v1/channels/{id}/finalize", post(crate::handlers::settlement::finalize));
        }
        Role::Hub => {
            // Hub inherits all provider routes
            router = router
                // Channel (provider subset)
                .route("/v1/channels/{id}/fund", post(crate::handlers::channel::fund_channel))
                .route("/v1/channels", get(crate::handlers::channel::list_channels))
                .route("/v1/channels/{id}", get(crate::handlers::channel::get_channel))
                // Payments (provider accept)
                .route("/v1/channels/{id}/cosign", post(crate::handlers::settlement::provider_cosign))
                .route("/v1/channels/{id}/accept-payment", post(crate::handlers::payment::accept_payment))
                .route("/v1/channels/{id}/accept-batch", post(crate::handlers::payment::accept_batch))
                // Settlement
                .route("/v1/channels/{id}/close", post(crate::handlers::settlement::cooperative_close))
                .route("/v1/channels/{id}/challenge", post(crate::handlers::settlement::trigger_challenge))
                .route("/v1/channels/{id}/submit-counter", post(crate::handlers::settlement::submit_counter))
                .route("/v1/channels/{id}/claim", post(crate::handlers::settlement::claim))
                .route("/v1/channels/{id}/finalize", post(crate::handlers::settlement::finalize))
                // Hub-specific routes
                .route("/v1/hub/register", post(crate::handlers::routing::register_hub))
                .route("/v1/hub/info", get(crate::handlers::routing::hub_info))
                .route("/v1/hub/metrics", post(crate::handlers::routing::update_metrics))
                .route("/v1/hub/list", get(crate::handlers::routing::list_hubs))
                .route("/v1/routes/find", post(crate::handlers::routing::find_routes_hub))
                .route("/v1/routes/add-edge", post(crate::handlers::routing::add_edge))
                .route("/v1/routes/refresh", post(crate::handlers::routing::refresh_graph))
                .route("/v1/multihop/relay", post(crate::handlers::multihop::relay_hop))
                .route("/v1/multihop/{id}", get(crate::handlers::multihop::get_payment));
        }
    }

    router.with_state(state).layer(cors)
}
