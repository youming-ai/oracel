# Deployment Guide

## Prerequisites

### System Requirements

| Resource | Minimum | Recommended |
|----------|---------|-------------|
| RAM | 512 MB | 1 GB |
| CPU | 1 core | 2+ cores |
| Disk | 100 MB | 1 GB (for logs) |
| Network | Stable internet | Low latency connection |

### Software Requirements

- **Rust**: 1.70+ (install via [rustup](https://rustup.rs/))
- **Git**: For cloning repository
- **systemd**: For service management (Linux)
- **tmux/screen**: Alternative process management

### Environment Setup

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version
cargo --version
```

---

## Installation

### 1. Clone Repository

```bash
git clone <repository-url>
cd oracel
```

### 2. Build Release Binary

```bash
# Build optimized release binary
cargo build --release

# Binaries: ./target/release/polybot (bot) and ./target/release/polybot-tools (CLI)
```

### 3. Create Configuration

Edit `config.json` with your settings.

### 4. Setup Environment Variables

```bash
# Create .env with your keys
cat > .env <<EOF
PRIVATE_KEY=your_private_key_here
ALCHEMY_KEY=your_alchemy_key_here
EOF
```

**Required for live mode**:
```bash
# .env
PRIVATE_KEY=0x...your_private_key...
ALCHEMY_KEY=...optional_alchemy_key...
```

**Security Note**: Never commit `.env` or `config.json` with real keys to version control.

---

## Running the Bot

### Development/Testing

```bash
# Run in development mode
cargo run

# Run with release optimizations
cargo run --release
```

### Production

```bash
# Run release binary directly
./target/release/polybot

# Or with logging
RUST_LOG=info ./target/release/polybot
```

### Using tmux (Simple)

```bash
# Create new session
tmux new -s polybot

# Run bot
cd /path/to/oracel
./target/release/polybot

# Detach: Ctrl+B, then D
# Reattach: tmux attach -t polybot
```

---

## systemd Service (Recommended)

### 1. Create Service File

Edit the provided template:

```bash
# Edit the service template
vim deploy/polybot.service
```

**Update paths** in `deploy/polybot.service`:

```ini
[Unit]
Description=Polymarket 5m Bot
After=network.target

[Service]
Type=simple
User=polybot
Group=polybot
WorkingDirectory=/home/polybot/oracel
ExecStart=/home/polybot/oracel/target/release/polybot
Restart=on-failure
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

### 2. Create User (Optional but Recommended)

```bash
# Create dedicated user
sudo useradd -m -s /bin/bash polybot

# Set permissions
sudo chown -R polybot:polybot /home/polybot/oracel
```

### 3. Install Service

```bash
# Copy service file
sudo cp deploy/polybot.service /etc/systemd/system/

# Reload systemd
sudo systemctl daemon-reload

# Enable service (start on boot)
sudo systemctl enable polybot

# Start service
sudo systemctl start polybot
```

### 4. Manage Service

```bash
# Check status
sudo systemctl status polybot

# View logs
sudo journalctl -u polybot -f

# Restart
sudo systemctl restart polybot

# Stop
sudo systemctl stop polybot

# Disable (don't start on boot)
sudo systemctl disable polybot
```

---

## Docker Deployment (Optional)

### Dockerfile

Create `Dockerfile`:

```dockerfile
FROM rust:1.75 as builder

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/polybot /app/polybot
COPY --from=builder /app/target/release/polybot-tools /app/polybot-tools
COPY config.json /app/config.json

# Create non-root user
RUN useradd -m -u 1000 bot && chown -R bot:bot /app
USER bot

CMD ["./polybot"]
```

### Build and Run

```bash
# Build image
docker build -t polybot .

# Run with environment
docker run -d \
  --name polybot \
  -v $(pwd)/.env:/app/.env:ro \
  -v $(pwd)/logs:/app/logs \
  polybot

# View logs
docker logs -f polybot
```

---

## Log Monitoring

### Real-time Log Monitor

Use the web dashboard:

```bash
# Paper mode (from dashboard/ directory)
cd dashboard && bun run dev

# Live mode
BOT_MODE=live bun run dev
```

### Log Files

Logs are stored in `logs/<mode>/`:

```
logs/
├── paper/
│   ├── bot.log          # Runtime logs
│   ├── trades.csv       # Trade history
│   └── balance          # Current balance
└── live/
    ├── bot.log
    ├── trades.csv
    └── balance
```

### Log Rotation

Set up log rotation to prevent disk fill:

```bash
# Create logrotate config
sudo tee /etc/logrotate.d/polybot << 'EOF'
/home/*/oracel/logs/*/bot.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
    copytruncate
}
EOF
```

---

## Backup and Recovery

### Recovery

The bot uses in-memory state only. On restart:
- Paper mode: Resumes with balance from `logs/paper/balance` (default $100)
- Live mode: Syncs balance from chain
- Pending positions: 5-minute markets settle before any realistic restart, so no persistence needed

### Critical Files to Backup

```bash
# Create backup archive
tar czf polybot_backup_$(date +%Y%m%d).tar.gz \
  config.json \
  .env \
  logs/*/balance
```

---

## Health Checks

### Process Health

```bash
# Check if running
pgrep -f polybot

# Or with systemd
systemctl is-active polybot
```

### Log Health Indicators

**Healthy signs**:
```
[INIT] Starting balance: $100.00
[STATUS] paper | BTC=$50000 balance=$1010.00 pnl=+10.00
[TRADE] DOWN @ 0.150 edge=35% BTC=$50000
```

**Warning signs**:
```
[RISK] Daily loss limit reached...  # High losses
[WS] Price receiver lagged by 1000 messages  # Connection issues
[SKIP] no_market_data  # API issues
```

**Error signs**:
```
Error: CLOB auth failed  # Authentication issue
Error: price_source.symbol must match...  # Config error
```

---

## Troubleshooting

### Bot Won't Start

**Check configuration**:
```bash
# Validate config
cargo run --release 2>&1 | head -20
```

**Common issues**:
- Invalid JSON syntax
- Missing required fields
- Out-of-range values
- Wrong symbol format

### WebSocket Connection Issues

**Symptoms**: No price updates, stale data

**Solutions**:
```bash
# Check internet connection
ping stream.binance.com

# Check firewall
sudo ufw status

# Try different price source
# Edit config.json: change source to "coinbase"
```

### Authentication Failures

**Symptoms**: "CLOB auth failed" errors

**Solutions**:
1. Verify `PRIVATE_KEY` in `.env`
2. Check key format (should start with `0x`)
3. Ensure account has USDC balance
4. Try deriving keys: `cargo run --release --bin polybot-tools -- --derive-keys`

### High Memory Usage

**Causes**:
- Long runtime without restart
- Excessive logging
- Too many pending positions

**Solutions**:
```bash
# Restart periodically (cron)
0 0 * * * systemctl restart polybot

# Reduce log level
RUST_LOG=warn ./target/release/polybot
```

---

## Security Best Practices

### 1. File Permissions

```bash
# Restrict sensitive files
chmod 600 .env
chmod 600 config.json

# Run as non-root user
sudo chown -R polybot:polybot /home/polybot/oracel
```

### 2. Secret Management

```bash
# Never commit secrets
echo ".env" >> .gitignore
echo "config.json" >> .gitignore

# Use environment variables for sensitive data
export PRIVATE_KEY=0x...
```

### 3. Network Security

- Use firewall to restrict outbound connections
- Run behind VPN if trading from restricted regions
- Monitor for unusual network activity

### 4. Key Rotation

```bash
# Generate new keys periodically
cargo run --release --bin polybot-tools -- --derive-keys

# Update .env with new keys
# Transfer funds to new address
```

---

## Performance Tuning

### Compile Optimizations

Already enabled in release build:
```bash
cargo build --release
```

### System Tuning

```bash
# Increase file descriptor limits
ulimit -n 65535

# Set in /etc/security/limits.conf
# polybot soft nofile 65535
# polybot hard nofile 65535
```

### Network Tuning

```bash
# Reduce TCP keepalive for faster reconnection
sudo sysctl -w net.ipv4.tcp_keepalive_time=60
sudo sysctl -w net.ipv4.tcp_keepalive_intvl=10
sudo sysctl -w net.ipv4.tcp_keepalive_probes=6
```

---

## Monitoring and Alerting

### Basic Monitoring Script

```bash
#!/bin/bash
# monitor.sh

LOG_FILE="logs/live/bot.log"
ALERT_EMAIL="admin@example.com"

# Check for errors
if tail -100 $LOG_FILE | grep -q "ERROR"; then
    echo "Errors detected in polybot" | mail -s "Polybot Alert" $ALERT_EMAIL
fi

# Check if running
if ! pgrep -f polybot > /dev/null; then
    echo "Polybot not running" | mail -s "Polybot Down" $ALERT_EMAIL
fi
```

### Prometheus Metrics (Advanced)

Add metrics export for Prometheus/Grafana monitoring:

```rust
// In main.rs, add metrics endpoint
use prometheus::{Counter, Gauge, Registry};

lazy_static! {
    static ref BALANCE_GAUGE: Gauge = Gauge::new(
        "polybot_balance", "Current balance"
    ).unwrap();
}
```

---

## Upgrade Process

### 1. Backup Current State

```bash
cp -r logs backups/logs_$(date +%Y%m%d)
cp config.json backups/config_$(date +%Y%m%d).json
```

### 2. Pull Updates

```bash
git fetch origin
git pull origin main
```

### 3. Rebuild

```bash
cargo build --release
```

### 4. Restart

```bash
sudo systemctl restart polybot
```

### 5. Verify

```bash
# Check logs
tail -f logs/live/bot.log

# Check status
sudo systemctl status polybot
```

---

## Production Checklist

Before running in production:

- [ ] Built in release mode (`--release`)
- [ ] Configuration validated
- [ ] Private key secured in `.env`
- [ ] Paper mode tested thoroughly
- [ ] Log rotation configured
- [ ] systemd service enabled
- [ ] Monitoring in place
- [ ] Backup strategy implemented
- [ ] Documentation reviewed
- [ ] Emergency stop procedure known
