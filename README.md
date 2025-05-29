# Ethereum Gas Estimator

A high-performance gas estimation service for Ethereum transactions, built with Rust and Axum. This service provides accurate gas units estimates for various transaction types, including simple transfers, contract interactions, and EIP-4844 blob transactions.

## 🌟 Features

- **High Performance**: Built with Rust and Axum for maximum throughput
- **Connection Pooling**: Efficient connection management to Ethereum nodes
- **Multiple Estimation Methods**:
  - Static calculation for simple transactions
  - RPC-based estimation for complex transactions
- **Monitoring**: Built-in health check endpoint

## 📊 Performance

The service is optimized for high throughput with:
- Async/await for non-blocking I/O
- Static gas calculation for simple transactions
- Connection pooling for efficient RPC communication
- Minimal runtime overhead

## 🚀 Quick Start

### Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/DezzK/gas-estimator.git
   cd gas-estimator
   ```

2. Run the service:
   ```bash
   cargo run --release
   ```

## ⚙️ Configuration

Configure the service using environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `ETH_RPC_URL` | Ethereum node RPC URL | `https://ethereum-rpc.publicnode.com` |

## 📚 API Reference

### Health Check
```http
GET /health
```

### Estimate Gas
```http
POST /api/estimate-gas
```

**Request Body:**
```json
{
  "from": "0x...",
  "to": "0x...",
  "value": "0x0",
  "data": "0x..."
}
```

**Response:**
```json
{
  "gas_limit": "0x5208",
  "method": "static"
}
```

## 💻 Example Usage

### Simple Transfer
```bash
curl -X POST http://localhost:3000/api/estimate-gas \
  -H "Content-Type: application/json" \
  -d '{"from":"0x0000000000000000000000000000000000000001","to":"0x0000000000000000000000000000000000000002","value":"0x1"}'
```

### Contract Interaction
```bash
curl -X POST http://localhost:3000/api/estimate-gas \
  -H "Content-Type: application/json" \
  -d '{"from":"0x0000000000000000000000000000000000000001","to":"0x6b175474e89094c44da98b954eedeac495271d0f","data":"0x70a082310000000000000000000000007b84eF0B14eEeDF32197bDD2B2B8CaCD17d9627c"}'
```

### Test

```bash
# Run all tests
cargo test
```
