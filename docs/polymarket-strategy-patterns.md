# Polymarket Trading Strategy Patterns

> Generated via librarian agent research (2026-03-19)

## 1. Strategy Categories

### A. Arbitrage Strategies

#### A1. Complete-Set (Yes/No) Arbitrage

The foundational prediction market arb. On Polymarket, `YES + NO` should always equal `$1.00`. When the combined cost drops below $1.00 (e.g., YES at $0.48 + NO at $0.47 = $0.95), buying both locks in guaranteed profit at resolution.

**Key constraints (2026):**
- Opportunity windows compressed to **<3 seconds** on liquid markets
- Requires sub-100ms execution; 15-20% cancellation rate from partial fills
- Average spread on binary markets: 0.3-0.8% (high-liquidity), 1-2% (low-liquidity)

One documented bot at `0x7a3f...` executed ~50,000 trades over 6 months, capturing average spreads of 1.2%, generating ~$152K net profit from $30-50K working capital. Win rate: 94.2%.

#### A2. Multi-Outcome Arbitrage

When markets have 3+ outcomes, implied probabilities should sum to 100%. When they sum to <100% (e.g., 96%), buying all outcomes guarantees a 4% return. Larger and more persistent mispricings than binary markets due to thinner per-outcome liquidity.

#### A3. Cross-Platform Arbitrage

Exploit price discrepancies between Polymarket (USDC/Polygon) and Kalshi (USD/CFTC-regulated). Same event priced at 62% on Polymarket, 58% on Kalshi. Hardest to execute due to non-atomic settlement across platforms.

#### A4. Temporal Arbitrage (Crypto Markets)

Exploits latency between confirmed spot prices on Binance/Coinbase and Polymarket's contract prices. BTC moves +0.3% on Binance, Polymarket UP contract still at $0.55, bot buys knowing true probability is ~85%+.

One documented bot turned $313 into $414,000 in a single month with a 98% win rate trading crypto Up/Down markets.

### B. Market Making

Provide liquidity on both sides, earn the bid-ask spread + Polymarket liquidity rewards.

As of Feb 2026, a subset of 5-minute and 15-minute crypto markets plus NCAAB/Serie A markets now feature a taker-fee + maker-rebate model, making midpoint-focused market making more attractive.

**Typical spread:** BUY YES at $0.48, SELL YES at $0.52 = 8.3% gross margin per round trip + rewards.

**Requires:** $10K+ capital, continuous order management, dynamic spread adjustment, inventory management.

### C. Mean Reversion / Contrarian

Bet against extreme sentiment. When a market overreacts to news, fade the move.

**This project's approach** (from STRATEGY.md):
- Buy `DOWN` when `mkt_up > 0.80` (overly bullish)
- Buy `UP` when `mkt_up < 0.20` (overly bearish)
- Uses `0.50` as fair value for 5-minute windows

This is a **sentiment-extreme contrarian** strategy -- it assumes short-term binary markets revert toward 50/50 when pricing gets extreme.

### D. Momentum / Trend Following

Enter positions when price has moved consistently in one direction, expecting continuation.

**Variant: Late-Window Momentum Snipe** -- enters in final 1-2 minutes of a 15-minute window when direction is statistically locked. Dangerous: buying at $0.95 requires 95%+ win rate to break even; real-world rates land at 85-90%.

### E. Event-Driven / Information Edge

Bots with faster news ingestion, sentiment analysis, or direct data feeds front-run probability shifts.

**Example: Shiva** -- Sports prediction bot using 6-factor probability model (home court, rest, form, injuries, H2H, media sentiment). Calculates edge via Kelly criterion, executes FOK orders when edge > 4%. Adaptive weights via Brier score calibration.

### F. New Market Sniper

Newly created markets are priced inefficiently. Trade within first 24-48 hours before automated participants calibrate.

### G. Liquidation Sweeper

When a large holder exits and crashes price below fair value, buy the temporary dip. Requires WebSocket orderbook monitoring.

## 2. Market Resolution & Settlement Timing

### Polymarket Resolution Mechanism

1. Market reaches resolution date/condition
2. UMA Optimistic Oracle proposes an outcome ($750 USDC bond, 2-hour dispute window)
3. If uncontested: resolves. If contested: Schelling-point arbitration by UMA token holders
4. Winning side pays $1.00 per share; losing side pays $0

### Settlement Timing Implications

- Settlement windows create predictable price dislocations as liquidity dries up pre-resolution
- Large position holders rush to exit before resolution: temporary price inefficiencies
- Spreads typically range wider in the final minutes before settlement
- 92% of traders lack execution speed to capitalize on these windows

## 3. Edge Detection Methods

### Probability Calibration

Compare your estimated true probability against market price. Edge = `your_probability - market_price`.

**Kelly Criterion:**
```
f* = (bp - q) / b
where b = (1 - Market_Price) / Market_Price
```
Fractional Kelly (0.25x-0.5x) recommended for real-world risk management.

### Structural Mispricing Detection

- **Yes/No sum violation**: `YES_ask + NO_ask < 1.00` = guaranteed arb
- **Multi-outcome sum violation**: Sum of all outcome prices != 100%
- **Cross-platform divergence**: Same event priced differently on Polymarket vs Kalshi

### Behavioral Bias Exploitation

- **Longshot bias**: Retail traders overpay for $0.01-$0.15 contracts (lottery effect)
- **Recency bias**: Prices overreact to recent news then mean-revert
- **Volume distortion**: High-attention markets can be *less* efficient than low-attention ones

### Data-Driven Edge

- **Temporal**: Spot price confirmed on exchange but not yet reflected on Polymarket
- **Informational**: Faster news ingestion (NLP, sentiment analysis, direct data feeds)
- **Statistical**: Team stats, injury reports, weather data priced more accurately than crowd sentiment

### This Project's Edge Detection

```
edge = fair_value - cheap_side_price
```
With threshold at 0.15 (15% edge required) and momentum filter to avoid stepping in front of real trends.

## 4. Risk Management Patterns

### Position-Level Controls

| Pattern | Description | This Bot |
|---------|-------------|----------|
| Position cap | Max USDC per trade | `max_position_size = 50.0` |
| Kelly sizing | Fractional Kelly based on win rate | Half-Kelly with edge multiplier |
| FOK orders | Fill-or-Kill -- no partial fills | Yes |
| One trade per window | Prevent overtrading in same market | Yes |

### Portfolio-Level Controls

| Pattern | Description | This Bot |
|---------|-------------|----------|
| Daily loss limit | Halt if daily P&L drops below threshold | `max_daily_loss_pct = 0.10` |
| Consecutive loss breaker | Circuit breaker after N losses | Stops after 8 consecutive |
| Loss pause | Temporary halt after losses | 1 min after 4-5 losses, 5 min after 6-7 |
| Cooldown | Minimum time between trades | `5000ms` |

### Execution Risk Controls

- **Partial fill monitoring**: Track when one leg of a two-sided trade fills but the other doesn't
- **Unpaired exposure limits**: Never hold directional risk from incomplete arb pairs
- **Gas cost monitoring**: Polygon gas spikes during high-activity events can erode margins
- **Rate limit management**: ~60 orders/min on Polymarket CLOB; exponential backoff for 429s

### Binary-Outcome-Specific Risks

- **Correlation**: Multiple positions on correlated events create concentrated risk
- **Resolution criteria mismatch**: Market may resolve differently than your model assumes
- **Settlement timing**: Capital locked until resolution; can't redeploy until market settles

## 5. Notable Public Implementations

| Project | Type | Language | Strategy |
|---------|------|----------|----------|
| **PBot1** | Live bot | Likely Python | Temporal arb on 15m crypto markets |
| **0x7a3f... bot** | On-chain | N/A | Yes/No arb, 50K trades, $152K profit |
| **Shiva** | Sports prediction | Python | 6-factor probability model + Kelly |
| **poly-maker** | Open-source | Python | Market making reference impl |
| **jtdoherty/arb-bot** | Open-source | Python | Cross-platform Polymarket/Kalshi arb |
| **aulekator/Polymarket-BTC-15-Minute-Trading-Bot** | Open-source | Python | 7-phase architecture, multi-signal |
| **dev-protocol/polymarket-arbitrage-bot** | Open-source | TypeScript | Dump-and-hedge on 15m Up/Down |
| **oracel (this project)** | Live | Rust | Sentiment-extreme contrarian on 5m BTC |

## 6. Competitive Landscape (2026)

**The market is compressing:**
- 2023-2024: Average arb spreads 3-5%, persisted for minutes
- Mid 2024-2025: Spreads compressed to 1-2% on major markets
- 2026: Binary spreads 0.3-0.8% on liquid markets; opportunities close in <3 seconds
- 14 of top 20 earning wallets on Polymarket are bots
- Sub-100ms institutional bots capture 73% of arb profits

**Where alpha still exists:**
1. Long-tail markets with thin order books
2. Event-driven spikes (seconds-long dislocations)
3. Multi-leg strategies combining arb with directional views
4. New market listings (first 24-48 hours)
5. Cross-platform discrepancies (Polymarket vs Kalshi)
6. **Sentiment extremes on short-duration markets** -- this project's niche

## 7. How This Bot Compares

| Dimension | This Bot | Industry Standard |
|-----------|----------|-------------------|
| **Strategy type** | Sentiment-extreme contrarian | Mostly temporal arb or market making |
| **Market** | 5-minute BTC Up/Down | 15-minute BTC/ETH/SOL (more common) |
| **Edge source** | Market sentiment divergence from 0.50 | Spot-exchange latency gap |
| **Sizing** | Half-Kelly with edge multiplier | Fractional Kelly (standard) |
| **Risk controls** | Consecutive loss breaker, daily % limit, cooldown | Similar patterns industry-wide |
| **Settlement** | Chainlink oracle (paper) / Gamma API (live) | Standard UMA/Gamma resolution |
| **Execution** | FOK orders, one trade per window | FOK standard; some trade multiple windows |

**Strengths:** Clean architecture, proper risk controls, momentum filter, Rust performance.

**Potential gaps vs. top bots:** No cross-platform coverage, no multi-market parallel trading, no adaptive weight calibration, fixed thresholds rather than dynamic edge adjustment.
