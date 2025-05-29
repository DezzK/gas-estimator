use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use reqwest::{Client as ReqwestClient, Url};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use web3::{
    Transport, Web3,
    api::Eth,
    transports::Http,
    types::{CallRequest, U256},
};

const BIND_ADDRESS: &str = "0.0.0.0:3000";
const DEFAULT_ETH_RPC_URL: &str = "https://ethereum-rpc.publicnode.com";
const RPC_TIMEOUT_SECS: u64 = 10;
const KEEP_ALIVE_SECS: u64 = 30;
const MAX_IDLE_CONNECTIONS: usize = 10;

// Gas constants based on Ethereum Yellow Paper and EIPs
const GAS_TX_BASE: u64 = 21000;
const GAS_TX_DATA_ZERO: u64 = 4;
const GAS_TX_DATA_NON_ZERO: u64 = 16;
const GAS_TX_CREATE: u64 = 32000;
const GAS_CODE_DEPOSIT: u64 = 200;

// EIP-4844: Shard Blob Transactions
const BLOB_TX_TYPE: u8 = 0x03;

#[derive(Debug, Serialize, Deserialize)]
pub struct GasEstimateResponse {
    pub gas_limit: U256,
    pub method: String, // "static" or "rpc"
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Custom error type for our API
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    InternalServerError(String),
}

/// Implement IntoResponse for our error type
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::BadRequest(error) => {
                (StatusCode::BAD_REQUEST, Json(ErrorResponse { error })).into_response()
            }
            ApiError::InternalServerError(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error }),
            )
                .into_response(),
        }
    }
}

// Our application state
#[derive(Clone)]
struct AppState<T: Transport + Send + Sync + 'static> {
    estimator: Arc<GasEstimator<T>>,
}

pub struct GasEstimator<T: Transport> {
    eth: Eth<T>,
}

impl<T: Transport> GasEstimator<T> {
    pub fn new(transport: T) -> Self {
        Self {
            eth: Web3::new(transport).eth(),
        }
    }

    /// Main estimation logic
    pub async fn estimate_gas(&self, tx: CallRequest) -> Result<GasEstimateResponse, ApiError> {
        // Determine estimation method
        if Self::is_blob_transaction(&tx) || self.needs_simulation(&tx) {
            // Use RPC for complex transactions
            let gas_limit = self
                .eth
                .estimate_gas(tx, None)
                .await
                .map_err(|e| ApiError::InternalServerError(format!("RPC call failed: {e}")))?;

            return Ok(GasEstimateResponse {
                gas_limit,
                method: "rpc".to_string(),
            });
        }

        // Use static calculation for simple transactions
        let gas_limit = self.calculate_static_gas(&tx).into();
        Ok(GasEstimateResponse {
            gas_limit,
            method: "static".to_string(),
        })
    }

    /// Check if this is a blob transaction (EIP-4844)
    fn is_blob_transaction(tx: &CallRequest) -> bool {
        tx.transaction_type == Some(BLOB_TX_TYPE.into())
    }

    /// Determines if transaction needs EVM simulation
    fn needs_simulation(&self, tx: &CallRequest) -> bool {
        if let Some(data) = &tx.data {
            // Contract calls with data (function or constructor calls)
            if !data.0.is_empty() {
                return true;
            }
            // Has value and data (might trigger receive/fallback functions)
            if let Some(value) = &tx.value {
                if !value.is_zero() {
                    return true;
                }
            }
        }

        false
    }

    /// Static gas calculation for simple transactions
    fn calculate_static_gas(&self, tx: &CallRequest) -> u64 {
        let mut gas = GAS_TX_BASE;

        // Contract creation vs regular transaction
        if tx.to.is_none() {
            gas += GAS_TX_CREATE;
        }

        // Calculate data gas (calldata)
        if let Some(data) = &tx.data {
            let data_bytes = &data.0;
            for &byte in data_bytes.iter() {
                if byte == 0 {
                    gas += GAS_TX_DATA_ZERO;
                } else {
                    gas += GAS_TX_DATA_NON_ZERO;
                }
            }

            // For contract creation, add code deposit cost
            if tx.to.is_none() {
                gas += data_bytes.len() as u64 * GAS_CODE_DEPOSIT;
            }
        }

        gas
    }
}

// API Handlers

/// Handles HTTP requests for gas estimation
/// POST: /api/estimate-gas
async fn estimate_gas_handler(
    State(state): State<AppState<Http>>,
    Json(payload): Json<CallRequest>,
) -> Result<Json<GasEstimateResponse>, ApiError> {
    state.estimator.estimate_gas(payload).await.map(Json)
}

/// Handles HTTP requests for health check
/// GET: /health
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "gas-estimator"
    }))
}

#[tokio::main]
async fn main() -> Result<(), String> {
    // Create a Reqwest client with connection pooling
    let reqwest_client = ReqwestClient::builder()
        .timeout(Duration::from_secs(RPC_TIMEOUT_SECS))
        .tcp_keepalive(Some(Duration::from_secs(KEEP_ALIVE_SECS)))
        .pool_max_idle_per_host(MAX_IDLE_CONNECTIONS) // Maximum number of idle connections per host
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    // Create Web3 transport with the configured client
    let rpc_url = Url::parse(
        &std::env::var("ETH_RPC_URL").unwrap_or_else(|_| DEFAULT_ETH_RPC_URL.to_string()),
    )
    .map_err(|e| format!("Failed to parse RPC URL: {e}"))?;
    let transport = Http::with_client(reqwest_client, rpc_url);

    // Create the gas estimator
    let estimator = GasEstimator::new(transport);
    let state = AppState {
        estimator: Arc::new(estimator),
    };

    // Set up CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build our application with a route
    let app = Router::new()
        .route("/api/estimate-gas", post(estimate_gas_handler))
        .route("/health", get(health_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(BIND_ADDRESS)
        .await
        .map_err(|e| format!("Failed to bind to address ({BIND_ADDRESS}): {e}"))?;

    println!("Running server on {BIND_ADDRESS}");

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use web3::{
        transports::test::TestTransport,
        types::{Address, Bytes, U256},
    };

    /// Helper function to create a mock transport that returns fixed gas values
    fn mock_transport() -> impl Transport {
        let mut mock = TestTransport::default();
        mock.set_response("0x5208".into()); // 21000 gas
        mock
    }

    /// Helper function to create an address
    fn address_from() -> Address {
        "0xc0ffee254729296a45a3885639AC7E10F9d54979"
            .parse()
            .unwrap()
    }

    /// Helper function to create an address
    fn address_to() -> Address {
        "0xc0ffee254729296a45a3885639AC7E10F9d54979"
            .parse()
            .unwrap()
    }

    /// Helper function to create a simple transfer request
    fn simple_transfer_request() -> CallRequest {
        CallRequest {
            from: Some(address_from()),
            to: Some(address_to()),
            value: Some(U256::one()),
            ..Default::default()
        }
    }

    #[test]
    fn test_calculate_static_gas_simple_transfer() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = simple_transfer_request();

        let gas = estimator.calculate_static_gas(&tx);
        assert_eq!(gas, GAS_TX_BASE);
    }

    #[test]
    fn test_calculate_static_gas_contract_creation() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = CallRequest {
            to: None, // Contract creation
            ..Default::default()
        };

        let gas = estimator.calculate_static_gas(&tx);
        assert_eq!(gas, GAS_TX_BASE + GAS_TX_CREATE);
    }

    #[test]
    fn test_calculate_static_gas_with_data() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = CallRequest {
            to: Some(address_to()),
            data: Some(Bytes::from(vec![0x01, 0x00, 0x02])), // 2 non-zero, 1 zero byte
            ..Default::default()
        };

        let gas = estimator.calculate_static_gas(&tx);
        assert_eq!(
            gas,
            GAS_TX_BASE + (GAS_TX_DATA_NON_ZERO * 2) + GAS_TX_DATA_ZERO
        );
    }

    #[test]
    fn test_needs_simulation_with_data() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = CallRequest {
            data: Some(Bytes::from(vec![0x01])),
            ..Default::default()
        };

        assert!(estimator.needs_simulation(&tx));
    }

    #[test]
    fn test_needs_simulation_with_value() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = CallRequest {
            data: Some(Bytes::default()),
            value: Some(U256::from(1)),
            ..Default::default()
        };

        assert!(estimator.needs_simulation(&tx));
    }

    #[tokio::test]
    async fn test_estimate_gas_static() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = simple_transfer_request();

        let result = estimator.estimate_gas(tx).await.unwrap();
        assert_eq!(result.gas_limit, GAS_TX_BASE.into());
        assert_eq!(result.method, "static");
    }

    #[tokio::test]
    async fn test_estimate_gas_rpc() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = CallRequest {
            data: Some(Bytes::from(vec![0x01])), // Forces RPC path
            ..Default::default()
        };

        let result = estimator.estimate_gas(tx).await.unwrap();
        assert_eq!(result.gas_limit, 21000.into()); // From mock response
        assert_eq!(result.method, "rpc");
    }

    #[tokio::test]
    async fn test_estimate_gas_blob_transaction() {
        let estimator = GasEstimator::new(mock_transport());
        let tx = CallRequest {
            transaction_type: Some(BLOB_TX_TYPE.into()),
            ..Default::default()
        };

        let result = estimator.estimate_gas(tx).await.unwrap();
        assert_eq!(result.method, "rpc");
    }
}
