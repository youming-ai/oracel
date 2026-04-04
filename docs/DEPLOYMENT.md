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

---

## Installation

### 1. Clone Repository

```bash
git clone <repository-url>
cd oracel
```

### 2. Build Release Binary

```bash
cargo build --release
# Binaries: ./target/release/polybot and ./target/release/polybot-tools
```

### 3. Create Configuration

Edit `config.toml` with your settings. A default config is auto-generated on first run.

### 4. Setup Environment Variables

```bash
# Create .env with your keys
cat > .env <<EOF
PRIVATE_KEY=your_private_key_here
ALCHEMY_KEY=your_alchemy_key_here
EOF
```

**Security Note**: Never commit `.env` to version control.

---

## Running the Bot

### Development/Testing

```bash
cargo run                # debug
cargo run --release      # release
```

### Production

```bash
./target/release/polybot
RUST_LOG=info ./target/release/polybot
```

### Using tmux (Simple)

```bash
tmux new -s polybot
cd /path/to/oracel
./target/release/polybot
# Detach: Ctrl+B, then D
# Reattach: tmux attach -t polybot
```

---

## systemd Service (Recommended)

### 1. Create Service File

Edit the provided template:

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

### 2. Install Service

```bash
sudo cp deploy/polybot.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable polybot
sudo systemctl start polybot
```

### 3. Manage Service

```bash
sudo systemctl status polybot    # check status
sudo journalctl -u polybot -f    # view logs
sudo systemctl restart polybot   # restart
sudo systemctl stop polybot      # stop
```

---

## Log Monitoring

### Web Dashboard

```bash
cd dashboard
bun run dev                    # paper mode
BOT_MODE=live bun run dev      # live mode
```

### Log Files

```
logs/
├── paper/
│   ├── bot.log
│   ├── trades.csv
│   └── balance
└── live/
    ├── bot.log
    ├── trades.csv
    ├── balance
    └── time_windows.json
```

### Log Rotation

```bash
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

The bot uses in-memory state only. On restart:
- Paper mode: Resumes with balance from `logs/paper/balance` (default $100)
- Live mode: Syncs balance from chain
- Pending positions: 5-minute markets settle before any realistic restart

### Critical Files to Backup

```bash
tar czf polybot_backup_$(date +%Y%m%d).tar.gz \
  config.toml \
  .env \
  logs/*/balance
```

---

## Health Checks

### Process Health

```bash
pgrep -f polybot
# Or with systemd
systemctl is-active polybot
```

### Log Health Indicators

**Healthy**:
```
[STATUS] live | BTC=$50000 bal=$1010.00 pnl=+10.00 | 10W/5L streak=2 | pending=0
[TRADE] DOWN @ 0.150 edge=35% BTC=$50000
```

**Warning**:
```
[RISK] Daily loss limit reached...
[WS] Price receiver lagged by 1000 messages
[SKIP] no_market_data
```

---

## Troubleshooting

### Bot Won't Start

```bash
# Validate config
cargo run --release 2>&1 | head -20
```

Common issues: invalid TOML syntax, out-of-range values, wrong symbol format.

### WebSocket Connection Issues

```bash
ping stream.binance.com
# Check firewall
sudo ufw status
```

### Authentication Failures

1. Verify `PRIVATE_KEY` in `.env`
2. Check key format (should start with `0x`)
3. Try deriving keys: `cargo run --release --bin polybot-tools -- --derive-keys`

---

## Security Best Practices

```bash
chmod 600 .env
chmod 600 config.toml
```

- Run as non-root user
- Never commit `.env` or private keys to version control
- Use firewall to restrict outbound connections

---

## Upgrade Process

```bash
# 1. Backup
cp -r logs backups/logs_$(date +%Y%m%d)
cp config.toml backups/config_$(date +%Y%m%d).toml

# 2. Pull and rebuild
git pull origin main
cargo build --release

# 3. Restart
sudo systemctl restart polybot

# 4. Verify
tail -f logs/live/bot.log
sudo systemctl status polybot
```

---

## Production Checklist

- [ ] Built in release mode (`--release`)
- [ ] Configuration validated (`config.toml`)
- [ ] Private key secured in `.env`
- [ ] Paper mode tested thoroughly
- [ ] Log rotation configured
- [ ] systemd service enabled
- [ ] Monitoring in place
