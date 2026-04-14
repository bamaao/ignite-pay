use crate::error::Result;
use crate::hub::{HubManager, HubMetrics};
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;
use std::collections::{HashMap, HashSet};

/// A single hop in a payment route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteHop {
    /// Public key of the hub for this hop.
    pub hub_pubkey: Pubkey,
    /// DID hash of the hub.
    pub hub_did_hash: [u8; 32],
    /// Fee charged by this hub (in lamports).
    pub fee: u64,
    /// Expected latency in milliseconds.
    pub latency_ms: u32,
    /// Available liquidity at this hub.
    pub liquidity: u64,
}

/// A complete route from source to destination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    /// Ordered list of hops.
    pub hops: Vec<RouteHop>,
    /// Total fee across all hops.
    pub total_fee: u64,
    /// Maximum latency across all hops.
    pub max_latency_ms: u32,
    /// Computed score (higher is better).
    pub score: f64,
    /// Whether all hops have sufficient liquidity.
    pub sufficient_liquidity: bool,
}

/// A route discovery request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    /// DID hash of the sender.
    pub from_did_hash: [u8; 32],
    /// DID hash of the receiver.
    pub to_did_hash: [u8; 32],
    /// Amount to route (in lamports).
    pub amount: u64,
    /// Token mint address.
    pub token_mint: Pubkey,
    /// Maximum number of hops allowed.
    pub max_hops: usize,
}

/// Service for discovering and scoring routes through the hub network.
pub struct RouteService {
    hub_manager: HubManager,
    /// Adjacency list: did_hash -> list of connected did_hashes.
    channel_graph: HashMap<[u8; 32], Vec<[u8; 32]>>,
}

impl std::fmt::Debug for RouteService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteService")
            .field("graph_nodes", &self.channel_graph.len())
            .finish()
    }
}

impl RouteService {
    /// Create a new RouteService with the given HubManager.
    pub fn new(hub_manager: HubManager) -> Self {
        Self {
            hub_manager,
            channel_graph: HashMap::new(),
        }
    }

    /// Add a directed channel edge between two hubs in the routing graph.
    ///
    /// This allows explicit topology control instead of assuming full mesh.
    /// Both directions can be added for bidirectional channels.
    pub fn add_channel_edge(&mut self, from: [u8; 32], to: [u8; 32]) {
        self.channel_graph
            .entry(from)
            .or_default()
            .push(to);
    }

    /// Refresh the channel graph from the hub registry.
    ///
    /// Builds an adjacency list based on explicit channel edges previously added.
    /// If no edges have been added, falls back to connecting hubs that both have
    /// liquidity (basic connectivity heuristic).
    pub fn refresh_graph(&mut self) -> Result<()> {
        // Only rebuild from scratch if no explicit edges exist
        if !self.channel_graph.is_empty() {
            return Ok(());
        }

        let hubs = self.hub_manager.list_hubs()?;

        // Fallback heuristic: hubs with available liquidity can route to each other.
        // Only connect hubs that actually have liquidity > 0.
        let liquid_hubs: Vec<[u8; 32]> = hubs.iter().filter(|did| {
            if let Ok(Some(m)) = self.hub_manager.get_metrics(**did) {
                m.available_liquidity > 0
            } else {
                false
            }
        }).copied().collect();

        for i in 0..liquid_hubs.len() {
            for j in 0..liquid_hubs.len() {
                if i != j {
                    self.channel_graph
                        .entry(liquid_hubs[i])
                        .or_default()
                        .push(liquid_hubs[j]);
                }
            }
        }
        Ok(())
    }

    /// Discover all routes for a given request.
    ///
    /// Uses DFS to find paths up to `max_hops` length from source to destination.
    /// Scores each route and returns them sorted by score (best first).
    pub fn discover_routes(&self, req: &RouteRequest) -> Result<Vec<Route>> {
        let mut routes = Vec::new();
        let mut visited = HashSet::new();
        let mut path: Vec<[u8; 32]> = Vec::new();

        self.dfs_routes(
            &req.from_did_hash,
            &req.to_did_hash,
            &req,
            &mut path,
            &mut visited,
            &mut routes,
        );

        // Sort by score descending
        routes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(routes)
    }

    fn dfs_routes(
        &self,
        current: &[u8; 32],
        destination: &[u8; 32],
        req: &RouteRequest,
        path: &mut Vec<[u8; 32]>,
        visited: &mut HashSet<[u8; 32]>,
        routes: &mut Vec<Route>,
    ) {
        path.push(*current);
        visited.insert(*current);

        if current == destination && path.len() > 1 {
            // Found a route
            if let Some(route) = self.build_route(path, req) {
                routes.push(route);
            }
        } else if path.len() - 1 < req.max_hops {
            if let Some(neighbors) = self.channel_graph.get(current) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        self.dfs_routes(neighbor, destination, req, path, visited, routes);
                    }
                }
            }
        }

        path.pop();
        visited.remove(current);
    }

    fn build_route(&self, path: &[[u8; 32]], req: &RouteRequest) -> Option<Route> {
        let mut hops = Vec::new();
        let mut total_fee = 0u64;
        let mut max_latency_ms = 0u32;
        let mut sufficient_liquidity = true;

        for did_hash in path {
            let hub = self.hub_manager.get_hub(*did_hash).ok()??;
            let metrics = self.hub_manager.get_metrics(*did_hash).ok()??;

            let fee = req.amount.saturating_mul(metrics.fee_rate_bps as u64) / 10000;
            total_fee = total_fee.saturating_add(fee);

            if metrics.avg_latency_ms > max_latency_ms {
                max_latency_ms = metrics.avg_latency_ms;
            }

            if metrics.available_liquidity < req.amount.saturating_add(total_fee) {
                sufficient_liquidity = false;
            }

            hops.push(RouteHop {
                hub_pubkey: hub.active_pubkey,
                hub_did_hash: *did_hash,
                fee,
                latency_ms: metrics.avg_latency_ms,
                liquidity: metrics.available_liquidity,
            });
        }

        // Score: 0.3*fee_score + 0.3*latency_score + 0.4*reliability_score
        let metrics_refs: Vec<_> = path.iter().filter_map(|d| {
            self.hub_manager.get_metrics(*d).ok().flatten()
        }).collect();

        let score = if !metrics_refs.is_empty() {
            Self::score_route_from_metrics(&metrics_refs, total_fee, max_latency_ms, req.amount)
        } else {
            0.0
        };

        Some(Route {
            hops,
            total_fee,
            max_latency_ms,
            score,
            sufficient_liquidity,
        })
    }

    /// Score a route based on fee, latency, and reliability metrics.
    ///
    /// Formula: 0.3*fee_score + 0.3*latency_score + 0.4*reliability_score
    pub fn score_route(
        path_metrics: &[&HubMetrics],
        amount: u64,
    ) -> f64 {
        if path_metrics.is_empty() {
            return 0.0;
        }

        let total_fee: u64 = path_metrics.iter()
            .map(|m| amount.saturating_mul(m.fee_rate_bps as u64) / 10000)
            .fold(0u64, |acc, f| acc.saturating_add(f));
        let max_latency: u32 = path_metrics.iter()
            .map(|m| m.avg_latency_ms)
            .max()
            .unwrap_or(0);

        let owned: Vec<HubMetrics> = path_metrics.iter().map(|m| (*m).clone()).collect();
        Self::score_route_from_metrics(&owned, total_fee, max_latency, amount)
    }

    fn score_route_from_metrics(
        metrics: &[HubMetrics],
        total_fee: u64,
        max_latency_ms: u32,
        amount: u64,
    ) -> f64 {
        if metrics.is_empty() {
            return 0.0;
        }

        // Fee score: lower is better, normalized to 0-1
        let fee_score = if total_fee == 0 {
            1.0
        } else if amount == 0 {
            0.0
        } else {
            1.0 / (1.0 + total_fee as f64 / amount as f64)
        };

        // Latency score: lower is better, per design doc §10.3.2
        let latency_score = if max_latency_ms == 0 {
            1.0
        } else {
            1.0 / (1.0 + max_latency_ms as f64 / 1000.0)
        };

        // Reliability score: min success rate across hops per design doc §10.3.2
        let min_success: f64 = metrics.iter()
            .map(|m| m.success_rate as f64 / 10000.0)
            .fold(f64::INFINITY, f64::min);

        0.3 * fee_score + 0.3 * latency_score + 0.4 * min_success
    }

    /// Select the best route from a list of discovered routes.
    /// DEV-13: use max_by to find highest score instead of relying on sort order.
    pub fn select_best_route(routes: &[Route]) -> Option<&Route> {
        routes.iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::HubLeaf;

    fn temp_db() -> sled::Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    fn setup_route_service() -> RouteService {
        let db = temp_db();
        let hub_mgr = HubManager::new(db).unwrap();

        let hub1_did = [1u8; 32];
        let hub2_did = [2u8; 32];
        let hub3_did = [3u8; 32];

        hub_mgr.register_hub(HubLeaf {
            hub_did_hash: hub1_did,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [0u8; 32],
            collateral: 10_000_000,
            platform_vc_hash: [0u8; 32],
            metrics_hash: [0u8; 32],
            slot_updated: 100,
        }).unwrap();

        hub_mgr.register_hub(HubLeaf {
            hub_did_hash: hub2_did,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [0u8; 32],
            collateral: 5_000_000,
            platform_vc_hash: [0u8; 32],
            metrics_hash: [0u8; 32],
            slot_updated: 100,
        }).unwrap();

        hub_mgr.register_hub(HubLeaf {
            hub_did_hash: hub3_did,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [0u8; 32],
            collateral: 8_000_000,
            platform_vc_hash: [0u8; 32],
            metrics_hash: [0u8; 32],
            slot_updated: 100,
        }).unwrap();

        hub_mgr.update_metrics(hub1_did, HubMetrics {
            online_rate: 9900, success_rate: 9950, avg_latency_ms: 30,
            total_routed: 1_000_000, total_transactions: 100, active_channels: 10,
            available_liquidity: 50_000_000, fee_rate_bps: 5,
        }).unwrap();

        hub_mgr.update_metrics(hub2_did, HubMetrics {
            online_rate: 9500, success_rate: 9800, avg_latency_ms: 80,
            total_routed: 500_000, total_transactions: 50, active_channels: 5,
            available_liquidity: 20_000_000, fee_rate_bps: 15,
        }).unwrap();

        hub_mgr.update_metrics(hub3_did, HubMetrics {
            online_rate: 9800, success_rate: 9900, avg_latency_ms: 40,
            total_routed: 800_000, total_transactions: 80, active_channels: 8,
            available_liquidity: 40_000_000, fee_rate_bps: 8,
        }).unwrap();

        let mut service = RouteService::new(hub_mgr);
        service.refresh_graph().unwrap();
        service
    }

    #[test]
    fn test_discover_routes() {
        let service = setup_route_service();
        let req = RouteRequest {
            from_did_hash: [1u8; 32],
            to_did_hash: [3u8; 32],
            amount: 1_000_000,
            token_mint: Pubkey::new_unique(),
            max_hops: 3,
        };

        let routes = service.discover_routes(&req).unwrap();
        assert!(!routes.is_empty(), "Should find at least one route");

        // Direct route (1 hop from 1 to 3 via hub graph)
        // and indirect routes through hub 2
    }

    #[test]
    fn test_score_route() {
        let metrics1 = HubMetrics {
            online_rate: 9900, success_rate: 9950, avg_latency_ms: 30,
            total_routed: 0, total_transactions: 0, active_channels: 0,
            available_liquidity: 0, fee_rate_bps: 5,
        };
        let metrics2 = HubMetrics {
            online_rate: 9000, success_rate: 8000, avg_latency_ms: 200,
            total_routed: 0, total_transactions: 0, active_channels: 0,
            available_liquidity: 0, fee_rate_bps: 50,
        };

        let score1 = RouteService::score_route(&[&metrics1], 1_000_000);
        let score2 = RouteService::score_route(&[&metrics2], 1_000_000);
        assert!(score1 > score2, "Better hub should have higher score");
        assert!(score1 > 0.0);
        assert!(score2 > 0.0);
    }

    #[test]
    fn test_select_best_route() {
        let route1 = Route {
            hops: vec![], total_fee: 100, max_latency_ms: 50,
            score: 0.9, sufficient_liquidity: true,
        };
        let route2 = Route {
            hops: vec![], total_fee: 500, max_latency_ms: 200,
            score: 0.5, sufficient_liquidity: true,
        };

        // DEV-13: best route is selected by max score regardless of order
        let routes = vec![route1.clone(), route2.clone()];
        let best = RouteService::select_best_route(&routes).unwrap();
        assert_eq!(best.score, 0.9);

        // Verify order doesn't matter
        let routes_reverse = vec![route2, route1];
        let best_reverse = RouteService::select_best_route(&routes_reverse).unwrap();
        assert_eq!(best_reverse.score, 0.9);
    }

    #[test]
    fn test_select_best_route_empty() {
        let routes: Vec<Route> = vec![];
        assert!(RouteService::select_best_route(&routes).is_none());
    }

    #[test]
    fn test_add_channel_edge_explicit_topology() {
        let db = temp_db();
        let hub_mgr = HubManager::new(db).unwrap();

        let hub1_did = [1u8; 32];
        let hub2_did = [2u8; 32];
        let hub3_did = [3u8; 32];

        hub_mgr.register_hub(HubLeaf {
            hub_did_hash: hub1_did,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [0u8; 32],
            collateral: 10_000_000,
            platform_vc_hash: [0u8; 32],
            metrics_hash: [0u8; 32],
            slot_updated: 100,
        }).unwrap();

        hub_mgr.register_hub(HubLeaf {
            hub_did_hash: hub2_did,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [0u8; 32],
            collateral: 5_000_000,
            platform_vc_hash: [0u8; 32],
            metrics_hash: [0u8; 32],
            slot_updated: 100,
        }).unwrap();

        hub_mgr.register_hub(HubLeaf {
            hub_did_hash: hub3_did,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [0u8; 32],
            collateral: 8_000_000,
            platform_vc_hash: [0u8; 32],
            metrics_hash: [0u8; 32],
            slot_updated: 100,
        }).unwrap();

        hub_mgr.update_metrics(hub1_did, HubMetrics {
            online_rate: 9900, success_rate: 9950, avg_latency_ms: 30,
            total_routed: 1_000_000, total_transactions: 100, active_channels: 10,
            available_liquidity: 50_000_000, fee_rate_bps: 5,
        }).unwrap();
        hub_mgr.update_metrics(hub2_did, HubMetrics {
            online_rate: 9500, success_rate: 9800, avg_latency_ms: 80,
            total_routed: 500_000, total_transactions: 50, active_channels: 5,
            available_liquidity: 20_000_000, fee_rate_bps: 15,
        }).unwrap();
        hub_mgr.update_metrics(hub3_did, HubMetrics {
            online_rate: 9800, success_rate: 9900, avg_latency_ms: 40,
            total_routed: 800_000, total_transactions: 80, active_channels: 8,
            available_liquidity: 40_000_000, fee_rate_bps: 8,
        }).unwrap();

        // Build explicit topology: 1 -> 2 -> 3 (linear, no direct 1->3)
        let mut service = RouteService::new(hub_mgr);
        service.add_channel_edge(hub1_did, hub2_did);
        service.add_channel_edge(hub2_did, hub3_did);
        service.refresh_graph().unwrap();

        let req = RouteRequest {
            from_did_hash: hub1_did,
            to_did_hash: hub3_did,
            amount: 1_000_000,
            token_mint: Pubkey::new_unique(),
            max_hops: 3,
        };

        let routes = service.discover_routes(&req).unwrap();
        assert!(!routes.is_empty(), "Should find route 1->2->3");

        // All routes must go through hub2 (no direct 1->3 edge)
        for route in &routes {
            assert_eq!(route.hops.len(), 3, "Route must be 1->2->3 (3 hops)");
            assert_eq!(route.hops[1].hub_did_hash, hub2_did, "Must go through hub2");
        }
    }

    #[test]
    fn test_score_route_uses_min_success_rate() {
        // DEV-7 fix: verify that a route with one bad hub scores lower
        // than a route with all good hubs (min vs avg)
        let good = HubMetrics {
            online_rate: 9900, success_rate: 9900, avg_latency_ms: 30,
            total_routed: 0, total_transactions: 0, active_channels: 0,
            available_liquidity: 0, fee_rate_bps: 5,
        };
        let bad = HubMetrics {
            online_rate: 5000, success_rate: 5000, avg_latency_ms: 30,
            total_routed: 0, total_transactions: 0, active_channels: 0,
            available_liquidity: 0, fee_rate_bps: 5,
        };

        // Route with [good, good] — min = 9900/10000 = 0.99
        let score_good = RouteService::score_route(&[&good, &good], 1_000_000);
        // Route with [good, bad] — min = 5000/10000 = 0.50
        let score_mixed = RouteService::score_route(&[&good, &bad], 1_000_000);

        assert!(score_good > score_mixed,
            "Route with all good hubs ({}) should score higher than mixed ({})",
            score_good, score_mixed);

        // With avg, the mixed route would score higher (avg = 0.745 vs 0.99).
        // With min, mixed scores lower (min = 0.50 vs 0.99).
        assert!(score_mixed < 0.8, "Mixed route score {} should be < 0.8 with min", score_mixed);
    }

    #[test]
    fn test_no_route_found() {
        let service = setup_route_service();
        let req = RouteRequest {
            from_did_hash: [99u8; 32], // Not in graph
            to_did_hash: [3u8; 32],
            amount: 1_000_000,
            token_mint: Pubkey::new_unique(),
            max_hops: 3,
        };

        let routes = service.discover_routes(&req).unwrap();
        assert!(routes.is_empty());
    }
}
