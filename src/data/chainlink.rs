//! RPC URL selection for Polygon.
//!
//! Paper mode: free public RPC
//! Live mode: Alchemy (via ALCHEMY_KEY env var)

use crate::config::TradingMode;

const PUBLIC_RPC: &str = "https://polygon-bor-rpc.publicnode.com";
const ALCHEMY_RPC: &str = "https://polygon-mainnet.g.alchemy.com/v2";

/// Pick RPC based on mode: live uses Alchemy (ALCHEMY_KEY env), paper uses public.
pub(crate) fn rpc_url(mode: TradingMode) -> String {
    if mode.is_live() {
        if let Ok(key) = std::env::var("ALCHEMY_KEY") {
            return format!("{}/{}", ALCHEMY_RPC, key);
        }
        tracing::warn!("[RPC] ALCHEMY_KEY not set, falling back to public RPC");
    }
    PUBLIC_RPC.to_string()
}
