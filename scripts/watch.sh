#!/bin/bash
# Real-time polybot monitor
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG="${1:-${ROOT}/logs/bot.log}"
SEC="${2:-3}"

# Use $'...' so colors are literal bytes, no escape interpretation needed
G=$'\033[32m'; R=$'\033[31m'; Y=$'\033[33m'; C=$'\033[36m'
B=$'\033[1m'; D=$'\033[2m'; N=$'\033[0m'

val() { echo "$2" | sed -n "s/.*${1}\([^ |]*\).*/\1/p"; }
ts() { echo "$1" | sed -n 's/^[0-9-]*T\([0-9:]*\)\..*/\1/p'; }
# printf %s — no escape interpretation, no % issues
out() { printf '%s\n' "$*"; }

clear; trap 'tput cnorm; exit' INT

while true; do
    tput cup 0 0; tput civis

    # ── Market ──
    MKT=$(grep '\[MKT\]' "$LOG" 2>/dev/null | tail -1)
    SLUG=$(echo "$MKT" | sed -n 's/.*\[MKT\] found \([^ ]*\).*/\1/p')
    END=$(echo "$MKT" | sed -n 's/.*ends \([-0-9]* [0-9:]* UTC\).*/\1/p')
    if [ -n "$END" ]; then
        ET=$(date -j -u -f "%Y-%m-%d %H:%M:%S" "${END% UTC}" "+%s" 2>/dev/null || echo 0)
        R_SEC=$(( ET - $(date -u "+%s") ))
        [ "$R_SEC" -gt 0 ] && TTL="${G}$((R_SEC/60))m$((R_SEC%60))s${N}" || TTL="${D}expired${N}"
    fi

    out "${B}Polymarket 5m Bot${N}  ${D}$(date +%H:%M:%S)${N}"
    out "${C}${SLUG:-discovering...}${N}  ends ${END:-?}  (${TTL:-?})"
    out ""

    # ── Latest BTC + Signal ──
    LAST=$(grep -E '\[SKIP\]|\[TRADE\]' "$LOG" 2>/dev/null | tail -1)
    PRICE=$(val 'BTC=\$' "$LAST")

    TRADE=$(grep '\[TRADE\]' "$LOG" 2>/dev/null | tail -1)
    if [ -n "$TRADE" ]; then
        DIR=$(echo "$TRADE" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
        EDGE=$(echo "$TRADE" | sed -n 's/.*edge=\([0-9]*\).*/\1/p')
        ENTRY=$(echo "$TRADE" | sed -n 's/.*@ *\([0-9.]*\).*/\1/p')
        out "BTC ${G}\$${PRICE}${N}  Last: ${B}${DIR}${N} @ ${ENTRY}  edge ${Y}${EDGE}%${N}"
    else
        out "BTC ${G}\$${PRICE:-?}${N}  ${D}no trades yet${N}"
    fi
    out ""

    # ── Balance (from balance.json + log) ──
    BALANCE=$(cat "${ROOT}/logs/balance" 2>/dev/null)
    BAL_LINE=$(grep '\[BAL\]' "$LOG" 2>/dev/null | tail -1)
    PNL=$(echo "$BAL_LINE" | sed -n 's/.*pnl=\([^ ]*\).*/\1/p')
    W=$(grep '\[SETTLED\]' "$LOG" 2>/dev/null | grep -c 'WIN'); W=${W:-0}
    L=$(grep '\[SETTLED\]' "$LOG" 2>/dev/null | grep -c 'LOSS'); L=${L:-0}

    out "Balance ${B}\$${BALANCE:-1000.00}${N}  PnL ${PNL:-0.00}  W/L ${G}${W}${N}/${R}${L}${N}"
    out ""

    # ── Recent Activity (last 8) ──
    out "${D}── Activity ──${N}"
    tail -300 "$LOG" 2>/dev/null | \
        grep -E '\[SETTLED\]|\[TRADE\]|\[MKT\] found' | tail -8 | \
        while IFS= read -r line; do
            TIME=$(ts "$line")
            if echo "$line" | grep -q '\[SETTLED\]'; then
                PNL_V=$(echo "$line" | sed -n 's/.*pnl=\([^ ]*\).*/\1/p')
                if echo "$line" | grep -q 'WIN'; then
                    out " ${D}${TIME}${N}  ${G}WIN${N}  ${PNL_V}"
                else
                    out " ${D}${TIME}${N}  ${R}LOSS${N} ${PNL_V}"
                fi
            elif echo "$line" | grep -q '\[TRADE\]'; then
                DIR=$(echo "$line" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
                out " ${D}${TIME}${N}  ${C}BUY${N}  ${DIR}"
            elif echo "$line" | grep -q '\[MKT\]'; then
                out " ${D}${TIME}  ↻ rotated${N}"
            fi
        done
    out ""

    sleep "$SEC"; tput ed
done
