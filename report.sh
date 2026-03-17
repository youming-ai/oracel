#!/bin/bash
LOG="/tmp/polybot.log"
PRINCIPAL=1000

strip_ansi() { sed 's/\x1b\[[0-9;]*m//g'; }

echo "🤖 Polymarket 5m Bot — 交易报告"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Status
if pgrep -x polybot > /dev/null; then
    PID=$(pgrep -x polybot)
    UP=$(ps -o etime= -p $PID | xargs)
    echo "🟢 运行中 | 已运行: $UP"
else
    echo "🔴 已停止"
fi

SIG=$(grep -c '\[SIGNAL\]' "$LOG" 2>/dev/null || echo 0)
TRD=$(grep -c 'TRADE:' "$LOG" 2>/dev/null || echo 0)
ROT=$(grep -c 'Market updated:' "$LOG" 2>/dev/null || echo 0)
echo "信号: $SIG | 交易: $TRD | 轮换: $ROT"

# BTC price range
BTC_FIRST=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | grep -oP 'BTC=\$\K[0-9]+' | head -1)
BTC_LAST=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | grep -oP 'BTC=\$\K[0-9]+' | tail -1)
if [ -n "$BTC_FIRST" ] && [ -n "$BTC_LAST" ]; then
    BTC_CHG=$(python3 -c "f=$BTC_FIRST; l=$BTC_LAST; print(f'$%s → $%s (%+.2f%%)' % (f, l, (l-f)/f*100))")
    echo "BTC: $BTC_CHG"
fi

# Trade direction
UP_CNT=$(grep 'TRADE:' "$LOG" 2>/dev/null | strip_ansi | grep -c 'TRADE: UP')
DN_CNT=$(grep 'TRADE:' "$LOG" 2>/dev/null | strip_ansi | grep -c 'TRADE: DOWN')
echo "方向: UP $UP_CNT / DOWN $DN_CNT"

# Warnings/Errors
WARN=$(grep 'WARN' "$LOG" 2>/dev/null | grep -v 'warning:' | wc -l | tr -d ' ')
ERR=$(grep 'ERROR' "$LOG" 2>/dev/null | wc -l | tr -d ' ')
echo "⚠️ 警告: $WARN | ❌ 错误: $ERR"

echo ""
echo "💰 收益率估算 (本金 \$$PRINCIPAL)"

# Python PnL calculation
python3 << 'PYEOF'
import re, sys

principal = 1000
with open('/tmp/polybot.log') as f:
    log = re.sub(r'\x1b\[[0-9;]*m', '', f.read())

trades = []
for line in log.split('\n'):
    if '=== TRADE:' in line:
        m = re.search(r'TRADE: (\w+).*?\$([0-9.]+) @ ([0-9.]+)', line)
        if m:
            p = float(m.group(3))
            if p > 0.001:
                trades.append({'side': m.group(1), 'size': float(m.group(2)), 'price': p})

if not trades:
    print("暂无交易数据")
    sys.exit()

up = [t for t in trades if t['side'] == 'UP']
dn = [t for t in trades if t['side'] == 'DOWN']
up_cost = sum(t['size'] for t in up)
up_shares = sum(t['size']/t['price'] for t in up)
dn_cost = sum(t['size'] for t in dn)
dn_shares = sum(t['size']/t['price'] for t in dn)
total_cost = up_cost + dn_cost

# Conservative estimate: 60% UP wins, 40% DOWN wins
up_pnl = up_shares * 0.60 - up_cost
dn_pnl = dn_shares * 0.40 - dn_cost
total_pnl = up_pnl + dn_pnl
yield_pct = (total_pnl / principal) * 100
roi = (total_pnl / total_cost * 100) if total_cost > 0 else 0

print(f"总投入: ${total_cost:.0f}")
print(f"估算PnL: ${total_pnl:+.0f}")
print(f"账户价值: ${principal + total_pnl:.0f}")
print(f"收益率: {yield_pct:+.1f}%")
print(f"投入ROI: {roi:+.0f}%")

# Buy price distribution
bins = [("<0.10", 0, 0.10), ("0.10-0.30", 0.10, 0.30), ("0.30-0.50", 0.30, 0.50), ("0.50-0.70", 0.50, 0.70), (">0.70", 0.70, 1.0)]
print("")
for label, lo, hi in bins:
    group = [t for t in trades if lo <= t['price'] < hi]
    if group:
        c = sum(t['size'] for t in group)
        s = sum(t['size']/t['price'] for t in group)
        p = s - c
        print(f"  {label}: {len(group)}笔 | 投入${c:.0f} | 若全赢+${p:.0f}")
PYEOF
