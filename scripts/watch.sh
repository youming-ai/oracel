#!/bin/bash
# Usage: scripts/watch.sh [mode] [refresh_seconds]
#   mode: paper (default) or live
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="${1:-paper}"
LOG="${ROOT}/logs/${MODE}/bot.log"
SEC="${2:-3}"

G=$'\033[32m'; R=$'\033[31m'; Y=$'\033[33m'; C=$'\033[36m'
B=$'\033[1m'; D=$'\033[2m'; N=$'\033[0m'; BR=$'\033[41;37m'

field() { echo "$1" | sed -n "s/.*${2}\([^ |]*\).*/\1/p"; }
logtime() { echo "$1" | sed -n 's/^[0-9-]*T\([0-9:]*\)\..*/\1/p'; }

clear; trap 'tput cnorm; exit' INT TERM

while true; do
    tput cup 0 0; tput civis

    STATUS=$(grep '\[STATUS\]' "$LOG" 2>/dev/null | tail -1 || true)
    MKT=$(grep '\[MKT\].*found\|cid=' "$LOG" 2>/dev/null | tail -1 || true)
    TRADE=$(grep '\[TRADE\]' "$LOG" 2>/dev/null | tail -1 || true)
    BALANCE=$(cat "${ROOT}/logs/${MODE}/balance" 2>/dev/null || echo "?")

    BTC=$(field "$STATUS" 'BTC=\$')
    PNL=$(field "$STATUS" 'pnl=')
    PENDING=$(field "$STATUS" 'pending=')
    TTL=$(field "$STATUS" 'ttl=')
    WL=$(echo "$STATUS" | sed -n 's/.*| \([0-9]*W\/[0-9]*L\).*/\1/p')
    STREAK=$(field "$STATUS" 'streak=')

    SLUG=$(echo "$MKT" | sed -n 's/.*\(btc-updown-5m[^ ]*\).*/\1/p')
    END=$(echo "$MKT" | sed -n 's/.*ends \([0-9-]* [0-9:]* UTC\).*/\1/p')
    END_SHORT=$(echo "$END" | sed -n 's/^[0-9-]* \([0-9:]*\) UTC/\1 UTC/p')

    if [ "$MODE" = "live" ]; then
        MODE_FMT="${BR} LIVE ${N}"
    else
        MODE_FMT="${D}PAPER${N}"
    fi

    if [ -n "$PNL" ]; then
        case "$PNL" in
            +*) PNL_FMT="${G}${PNL}${N}" ;;
            -*)  PNL_FMT="${R}${PNL}${N}" ;;
            *)   PNL_FMT="${PNL}" ;;
        esac
    else
        PNL_FMT="${D}0.00${N}"
    fi

    if [ -n "$BTC" ] && [ "$BTC" != "0" ]; then
        BTC_FMT=$(printf "%'d" "${BTC%.*}" 2>/dev/null || echo "$BTC")
    else
        BTC_FMT="${D}waiting${N}"
    fi

    printf '%s  %s  %s\n\n' "${B}POLYBOT${N}" "${MODE_FMT}" "${D}$(date +%H:%M:%S)${N}"

    printf '  %s  $%s %28s\n' "${B}BTC${N}" "${BTC_FMT}" "${C}${SLUG:-discovering...}${N}"
    printf '  %s  %s %32s\n\n' "${B}TTL${N}" "${G}${TTL:-?}${N}" "${D}ends ${END_SHORT:-?}${N}"

    printf '  Balance  %s        P&L  %s\n' "${B}\$${BALANCE}${N}" "${PNL_FMT}"
    printf '  Record   %s      Streak  %s\n' "${G}${WL:-0W/0L}${N}" "${Y}${STREAK:-0}${N}"
    [ -n "$PENDING" ] && [ "$PENDING" != "0" ] && printf '  %s\n' "${Y}⏳ ${PENDING} pending${N}"
    echo ""

    printf '%s\n' "${D}── last trade ──${N}"
    if [ -n "$TRADE" ]; then
        T_TIME=$(logtime "$TRADE")
        T_DIR=$(echo "$TRADE" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
        T_PRICE=$(echo "$TRADE" | sed -n 's/.*@ *\([0-9.]*\).*/\1/p')
        T_EDGE=$(echo "$TRADE" | sed -n 's/.*edge=\([0-9]*\).*/\1/p')
        if [ "$T_DIR" = "UP" ]; then
            DIR_CLR="${G}▲ UP${N}"
        else
            DIR_CLR="${R}▼ DOWN${N}"
        fi
        printf '  %s  BUY %s @ %s  edge %s\n' "${D}${T_TIME}${N}" "${DIR_CLR}" "${B}${T_PRICE}${N}" "${Y}${T_EDGE}%${N}"
    else
        printf '  %s\n' "${D}no trades yet${N}"
    fi
    echo ""

    printf '%s\n' "${D}── activity ──${N}"
    ACTIVITY=$(tail -500 "$LOG" 2>/dev/null | grep -E '\[SETTLED\]|\[TRADE\]|\[MKT\].*found' | tail -6 || true)
    if [ -n "$ACTIVITY" ]; then
        echo "$ACTIVITY" | while IFS= read -r line; do
            TIME=$(logtime "$line")
            if echo "$line" | grep -q '\[SETTLED\]'; then
                S_PNL=$(echo "$line" | sed -n 's/.*pnl=\([^ ]*\).*/\1/p')
                if echo "$line" | grep -q 'WIN'; then
                    printf '  %s  %s  %s\n' "${D}${TIME}${N}" "${G}✓ WIN${N} " "${G}${S_PNL}${N}"
                else
                    printf '  %s  %s  %s\n' "${D}${TIME}${N}" "${R}✗ LOSS${N}" "${R}${S_PNL}${N}"
                fi
            elif echo "$line" | grep -q '\[TRADE\]'; then
                A_DIR=$(echo "$line" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
                A_PRICE=$(echo "$line" | sed -n 's/.*@ *\([0-9.]*\).*/\1/p')
                printf '  %s  %s  %s @ %s\n' "${D}${TIME}${N}" "${C}● BUY${N} " "${A_DIR}" "${A_PRICE}"
            elif echo "$line" | grep -q '\[MKT\]'; then
                printf '  %s  %s\n' "${D}${TIME}${N}" "${D}↻ market rotated${N}"
            fi
        done
    else
        printf '  %s\n' "${D}no activity yet${N}"
    fi
    echo ""

    ERRORS=$(tail -500 "$LOG" 2>/dev/null | grep -E '\[EXEC\].*failed|\[EXEC\].*FOK' | tail -3 || true)
    if [ -n "$ERRORS" ]; then
        printf '%s\n' "${D}── errors ──${N}"
        echo "$ERRORS" | while IFS= read -r line; do
            E_TIME=$(logtime "$line")
            if echo "$line" | grep -q 'FOK'; then
                E_PRICE=$(echo "$line" | sed -n 's/.*at \([0-9.]*\).*/\1/p')
                printf '  %s  %s @ %s\n' "${D}${E_TIME}${N}" "${Y}FOK rejected${N}" "${E_PRICE}"
            else
                E_MSG=$(echo "$line" | sed 's/.*order failed: //' | cut -c1-45)
                printf '  %s  %s\n' "${D}${E_TIME}${N}" "${R}${E_MSG}${N}"
            fi
        done
        echo ""
    fi

    sleep "$SEC"; tput ed
done
