mod types;
mod vrf_engine;
mod settlement_engine;
mod storage;

use types::{CoinflipRequest, CoinflipResponse, VfError};
use settlement_engine::SettlementEngine;
use storage::Storage;
use vrf_engine::VrfEngine;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::{
    cors::CorsLayer, 
    trace::TraceLayer,
    compression::CompressionLayer,
    timeout::TimeoutLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::time::Duration;

#[derive(Clone)]
struct AppState {
    vrf_engine: Arc<VrfEngine>,
    settlement_engine: Arc<SettlementEngine>,
    storage: Arc<Storage>,
}

async fn coinflip(
    State(state): State<AppState>,
    Json(req): Json<CoinflipRequest>,
) -> Result<Json<CoinflipResponse>, StatusCode> {
    let start = std::time::Instant::now();
    let engine = state.vrf_engine.clone();
    let req_clone = req.clone(); // Clone for settlement
    
    let result = tokio::task::spawn_blocking(move || engine.process_coinflip(&req)).await;
    
    match result {
        Ok(vrf_result) => {
            match vrf_result {
                Ok(mut response) => {
                    response.processing_time_ms = start.elapsed().as_millis() as u64;
                    
                    // Enqueue bet for settlement processing (non-blocking)
                    if let Err(e) = state.settlement_engine.enqueue_bet_fast(&response, &req_clone) {
                        tracing::warn!("Failed to enqueue bet for settlement: {}", e);
                    }
                    
                    Ok(Json(response))
                }
                Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
        Err(e) => {
            tracing::error!("Coinflip processing failed: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "vfnode",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "tokio-multi-thread",
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }))
}

async fn node_info(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "node_pubkey": state.vrf_engine.node_pubkey(),
        "service": "vfnode",
        "version": env!("CARGO_PKG_VERSION"),
        "supported_games": ["coinflip"],
        "max_concurrent": num_cpus::get(),
        "features": ["multi-threaded", "async", "optimized", "settlement-engine"]
    }))
}

async fn settlement_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let stats = state.settlement_engine.get_stats().await;
    Json(serde_json::to_value(stats).unwrap_or_default())
}

async fn settlement_summary(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match state.storage.get_settlement_summary().await {
        Ok(summary) => Ok(Json(summary)),
        Err(e) => {
            tracing::error!(error = %e, "Failed to get settlement summary");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Failed to get settlement summary".to_string()))
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Enhanced tracing for performance monitoring
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vfnode=info,tower_http=info".into())
        )
        .with(tracing_subscriber::fmt::layer()
            .with_target(false)
            .compact()
        )
        .init();

    // Initialize storage
    let storage = Arc::new(Storage::new("sqlite:./vfnode.db").await?);

    // Initialize VRF engine
    let vrf_engine = Arc::new(VrfEngine::new());
    
    // Initialize settlement engine with high-performance configuration
    let settlement_engine = SettlementEngine::new(
        storage.pool(),
        50,  // batch_size: Process up to 50 bets per settlement
        10   // processing_interval_seconds: Process every 10 seconds (for testing)
    )?;
    
    tracing::info!(
        node_pubkey = vrf_engine.node_pubkey(),
        worker_threads = num_cpus::get(),
        settlement_interval_seconds = 10,
        settlement_batch_size = 50,
        "VF Node with Settlement Engine initializing"
    );

    let state = AppState { 
        vrf_engine,
        settlement_engine,
        storage,
    };

    // Optimized router with settlement endpoints
    let app = Router::new()
        .route("/coinflip", post(coinflip))
        .route("/health", get(health))
        .route("/info", get(node_info))
        .route("/settlement/stats", get(settlement_stats))
        .route("/settlement/summary", get(settlement_summary))
        .layer(CompressionLayer::new()) // Compress responses
        .layer(TimeoutLayer::new(Duration::from_secs(5))) // Request timeout
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Optimized server configuration
    let port = std::env::var("PORT").unwrap_or_else(|_| "3001".to_string());
    let addr = format!("0.0.0.0:{}", port);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    // Enhanced startup info
    tracing::info!(
        addr = %addr,
        worker_threads = num_cpus::get(),
        "VF Node with Settlement Engine server starting"
    );
    
    println!("🚀 VF Node running on http://{}", addr);
    println!("⚡ Multi-threaded with {} worker threads", num_cpus::get());
    println!("🎯 Optimized for high-throughput, low-latency");
    println!("🏦 Settlement engine: 50 bets per batch, 10 second intervals");
    println!("📊 Settlement stats: http://{}/settlement/stats", addr);
    
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown");
}