#!/bin/bash
LOG="/tmp/polybot.log"

strip_ansi() { sed 's/\x1b\[[0-9;]*m//g'; }

echo "═══════════════════════════════════════════════════════════════"
echo "  🤖 Polymarket 5m Bot — Status Report"
echo "═══════════════════════════════════════════════════════════════"

# Runtime
if pgrep -x polybot > /dev/null; then
    PID=$(pgrep -x polybot)
    UPTIME=$(ps -o etime= -p $PID | xargs)
    echo "  Status: 🟢 RUNNING (PID $PID, uptime $UPTIME)"
else
    echo "  Status: 🔴 STOPPED"
fi
echo ""

# Stats
SIGNALS=$(grep -c '\[SIGNAL\]' "$LOG" 2>/dev/null || echo 0)
TRADES=$(grep -c 'TRADE:' "$LOG" 2>/dev/null || echo 0)
SKIPS=$(grep -c 'NO TRADE' "$LOG" 2>/dev/null || echo 0)
ROTATIONS=$(grep -c 'Market updated:' "$LOG" 2>/dev/null || echo 0)
echo "  📊 Signals: $SIGNALS  |  Trades: $TRADES  |  Skips: $SKIPS  |  Rotations: $ROTATIONS"
echo ""

# Current market
LAST_SIGNAL=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | tail -1 | strip_ansi)
if [ -n "$LAST_SIGNAL" ]; then
    BTC=$(echo "$LAST_SIGNAL" | grep -oP 'BTC=\$\K[0-9]+')
    if [ -z "$BTC" ]; then
        BTC=$(echo "$LAST_SIGNAL" | grep -oP 'Bin=\$\K[0-9]+')
    fi
    EDGE=$(echo "$LAST_SIGNAL" | grep -oP 'Edge: \K[0-9.]+')
    PUP=$(echo "$LAST_SIGNAL" | grep -oP 'P\(UP\)=\K[0-9]+')
    PDN=$(echo "$LAST_SIGNAL" | grep -oP 'P\(DN\)=\K[0-9]+')
    YES=$(echo "$LAST_SIGNAL" | grep -oP 'Yes=\K[0-9.]+')
    NO=$(echo "$LAST_SIGNAL" | grep -oP 'No=\K[0-9.]+')
    REGIME=$(echo "$LAST_SIGNAL" | grep -oP 'Regime=\K[A-Za-z]+')
    MARKET=$(grep 'Selected market:' "$LOG" 2>/dev/null | tail -1 | strip_ansi | grep -oP 'market: \K\S+')
    ENDS=$(grep 'Selected market:' "$LOG" 2>/dev/null | tail -1 | strip_ansi | grep -oP 'ends: \K\S+')
    
    echo "  📈 Current Market: $MARKET"
    echo "     Ends: $ENDS UTC"
    echo ""
    echo "  💰 BTC Price:  \$${BTC}"
    echo "  📊 Model:      P(UP)=${PUP}%  P(DN)=${PDN}%"
    echo "  🎯 Market:     Yes=${YES}  No=${NO}"
    echo "  ⚡ Edge:       ${EDGE}%  (${REGIME})"
fi
echo ""

# Last 10 trades
echo "  ── Recent Trades ──────────────────────────────────────────"
grep 'TRADE:' "$LOG" 2>/dev/null | tail -10 | strip_ansi | while read -r line; do
    TIME=$(echo "$line" | grep -oP '^\S+T\K[0-9:]+' | cut -c1-8)
    SIDE=$(echo "$line" | grep -oP 'TRADE: \K\w+')
    EDGE=$(echo "$line" | grep -oP 'edge: \K[0-9.]+')
    PRICE=$(echo "$line" | grep -oP '@ \K[0-9.]+')
    echo "     ${TIME}  ${SIDE} @ ${PRICE}  (edge ${EDGE}%)"
done
echo ""

# Edge distribution
echo "  ── Edge Distribution (all trades) ─────────────────────────"
grep '\[SIGNAL\]' "$LOG" 2>/dev/null | grep -oP 'Edge: \K[0-9.]+' | \
    awk '{
        if ($1 >= 50) bucket="50%+ "
        else if ($1 >= 30) bucket="30-50"
        else if ($1 >= 10) bucket="10-30"
        else bucket="<10  "
        count[bucket]++
        total+=$1; n++
        if ($1>max) max=$1
    }
    END {
        if (n==0) exit
        printf "     Avg: %.1f%%  Max: %.1f%%  Total: %d signals\n", total/n, max, n
        for (b in count) printf "     %s  %s\n", b, count[b]
    }' | sort -t' ' -k1

# Warnings
WARNINGS=$(grep 'WARN' "$LOG" 2>/dev/null | grep -v 'warning:' | wc -l | tr -d ' ')
ERRORS=$(grep 'ERROR' "$LOG" 2>/dev/null | grep -v 'error\[' | wc -l | tr -d ' ')
if [ "${WARNINGS:-0}" -gt 0 ] 2>/dev/null; then
    echo ""
    echo "  ⚠️  Warnings: $WARNINGS"
fi
if [ "${ERRORS:-0}" -gt 0 ] 2>/dev/null; then
    echo ""
    echo "  ❌ Errors: $ERRORS"
fi

echo "═══════════════════════════════════════════════════════════════"
