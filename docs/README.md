# Documentation Index

Welcome to the Polymarket 5m Bot documentation.

## Quick Start

1. **[Configuration Guide](CONFIGURATION.md)** - Set up your bot
2. **[Deployment Guide](DEPLOYMENT.md)** - Deploy to production
3. **[Trading Strategy](STRATEGY.md)** - Understand the strategy

## Reference Documentation

- **[Architecture Overview](ARCHITECTURE.md)** - System design and data flow
- **[Module Documentation](MODULES.md)** - Detailed API for each module
- **[API Documentation](API.md)** - Internal APIs and data structures

## Getting Started

New to the project? Start here:

1. Read [STRATEGY.md](STRATEGY.md) to understand the trading approach
2. Review [ARCHITECTURE.md](ARCHITECTURE.md) for system overview
3. Follow [CONFIGURATION.md](CONFIGURATION.md) to set up your bot
4. Use [DEPLOYMENT.md](DEPLOYMENT.md) to deploy

## Document Structure

```
docs/
├── README.md              # This file
├── STRATEGY.md            # Trading strategy documentation
├── ARCHITECTURE.md        # System architecture
├── MODULES.md            # Module-by-module documentation
├── API.md                # API reference
├── CONFIGURATION.md      # Configuration guide
└── DEPLOYMENT.md         # Deployment guide
```

## Key Concepts

### Pipeline Architecture

The bot follows a 5-stage pipeline:

1. **PriceSource** - Real-time price ingestion
2. **Signal** - Market opportunity detection
3. **Decider** - Trade decision and risk management
4. **Executor** - Order execution (paper/live)
5. **Settler** - Position settlement and PnL

### Risk Control Modes

- **Advisory** (`enforce_limits: false`): Log warnings, continue trading
- **Strict** (`enforce_limits: true`): Block trading on violations

### Trading Modes

- **Paper**: Simulated trading for testing
- **Live**: Real trading with actual funds

## Support

For issues and questions:

1. Check relevant documentation section
2. Review [API.md](API.md) for technical details
3. See [DEPLOYMENT.md](DEPLOYMENT.md) troubleshooting section

## Contributing

When adding features:

1. Update relevant documentation
2. Add examples to [API.md](API.md)
3. Update [CONFIGURATION.md](CONFIGURATION.md) if adding settings
4. Keep documentation in sync with code
