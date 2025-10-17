mod types;
mod vrf_engine;

use types::{CoinflipRequest, CoinflipResponse};
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
}

async fn coinflip(
    State(state): State<AppState>,
    Json(req): Json<CoinflipRequest>,
) -> Result<Json<CoinflipResponse>, StatusCode> {
    let start = std::time::Instant::now();
    let engine = state.vrf_engine.clone();
    
    let result = tokio::task::spawn_blocking(move || engine.process_coinflip(&req)).await;
    
    match result {
        Ok(response) => {
            match response {
                Ok(mut coinflip_response) => {
                    coinflip_response.processing_time_ms = start.elapsed().as_millis() as u64;
                    Ok(Json(coinflip_response))
                }
                Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
        Err(_) => {
            tracing::error!("Coinflip processing failed");
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
        "max_concurrent": 10,
        "features": ["multi-threaded", "async", "optimized"]
    }))
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

    // Initialize VRF engine
    let vrf_engine = Arc::new(VrfEngine::new());
    
    tracing::info!(
        node_pubkey = vrf_engine.node_pubkey(),
        worker_threads = num_cpus::get(),
        "VF Node initializing"
    );

    let state = AppState { vrf_engine };

    // Optimized router with performance middleware
    let app = Router::new()
        .route("/coinflip", post(coinflip))
        .route("/health", get(health))
        .route("/info", get(node_info))
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
        "VF Node server starting"
    );
    
    println!("🚀 VF Node running on http://{}", addr);
    println!("⚡ Multi-threaded with {} worker threads", num_cpus::get());
    println!("🎯 Optimized for high-throughput, low-latency");
    
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}