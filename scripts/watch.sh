#!/bin/bash
# Usage: scripts/watch.sh [mode] [refresh_seconds]
#   mode: paper (default) or live
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="${1:-paper}"
LOG="${ROOT}/logs/${MODE}/bot.log"
STATE="${ROOT}/logs/${MODE}/state.json"
SEC="${2:-3}"

G=$'\033[32m'; R=$'\033[31m'; Y=$'\033[33m'; C=$'\033[36m'; M=$'\033[35m'
B=$'\033[1m'; D=$'\033[2m'; N=$'\033[0m'

field() { echo "$1" | sed -n "s/.*${2}\([^ |]*\).*/\1/p"; }
logtime() { echo "$1" | sed -n 's/^[0-9-]*T\([0-9:]*\)\..*/\1/p'; }

progress_bar() {
    local cur=$1 total=$2 width=${3:-20}
    if [ "$total" -le 0 ]; then printf '%*s' "$width" ''; return; fi
    local pct=$(( cur * 100 / total ))
    [ "$pct" -gt 100 ] && pct=100
    local filled=$(( pct * width / 100 ))
    local empty=$(( width - filled ))
    local bar=""
    local i
    for ((i=0; i<filled; i++)); do bar+="#"; done
    for ((i=0; i<empty; i++)); do bar+="-"; done
    printf '%s' "$bar"
}

tput smcup; clear; tput civis
trap 'tput cnorm; tput rmcup' EXIT
trap 'exit' INT TERM

while true; do
    tput cup 0 0

    STATUS=$(grep '\[STATUS\]' "$LOG" 2>/dev/null | tail -1 || true)
    MKT=$(grep -E '\[MKT\].*(found|cid=|ends)' "$LOG" 2>/dev/null | tail -1 || true)
    TRADE=$(grep '\[TRADE\]' "$LOG" 2>/dev/null | tail -1 || true)
    BALANCE=$(cat "${ROOT}/logs/${MODE}/balance" 2>/dev/null || true)
    BALANCE="${BALANCE:-?}"

    BTC=$(field "$STATUS" 'BTC=\$')
    PNL=$(field "$STATUS" 'pnl=')
    PENDING=$(field "$STATUS" 'pending=')
    TTL=$(field "$STATUS" 'ttl=')
    WL=$(echo "$STATUS" | sed -n 's/.*| \([0-9]*W\/[0-9]*L\).*/\1/p')
    STREAK=$(field "$STATUS" 'streak=')

    SLUG=$(echo "$MKT" | sed -n 's/.*\(btc-updown-5m[^ ]*\).*/\1/p')
    END=$(echo "$MKT" | sed -n 's/.*ends \([0-9-]* [0-9:]* UTC\).*/\1/p')
    END_SHORT=$(echo "$END" | sed -n 's/^[0-9-]* \([0-9:]*\) UTC/\1 UTC/p')

    WINS=$(echo "$WL" | sed -n 's/\([0-9]*\)W.*/\1/p')
    LOSSES=$(echo "$WL" | sed -n 's/.*\/\([0-9]*\)L/\1/p')
    WINS="${WINS:-0}"; LOSSES="${LOSSES:-0}"
    TOTAL_TRADES=$(( WINS + LOSSES ))

    if [ "$TOTAL_TRADES" -gt 0 ]; then
        WIN_RATE=$(awk "BEGIN { printf \"%.1f\", ($WINS / $TOTAL_TRADES) * 100 }")
    else
        WIN_RATE=""
    fi

    DAILY_PNL_RAW=""
    if [ -f "$STATE" ]; then
        DAILY_PNL_RAW=$(sed -n 's/.*"daily_pnl":"\([^"]*\)".*/\1/p' "$STATE" 2>/dev/null || true)
    fi

    FIRST_TS=$(head -1 "$LOG" 2>/dev/null | sed -n 's/^\([0-9-]*T[0-9:]*\).*/\1/p' || true)
    UPTIME_FMT="?"
    if [ -n "$FIRST_TS" ]; then
        if date -j >/dev/null 2>&1; then
            FIRST_EPOCH=$(date -j -f "%Y-%m-%dT%H:%M:%S" "$FIRST_TS" "+%s" 2>/dev/null || echo 0)
        else
            FIRST_EPOCH=$(date -d "$FIRST_TS" "+%s" 2>/dev/null || echo 0)
        fi
        NOW_EPOCH=$(date "+%s")
        if [ "$FIRST_EPOCH" -gt 0 ]; then
            UPTIME_S=$(( NOW_EPOCH - FIRST_EPOCH ))
            UPTIME_H=$(( UPTIME_S / 3600 ))
            UPTIME_M=$(( (UPTIME_S % 3600) / 60 ))
            if [ "$UPTIME_H" -gt 0 ]; then
                UPTIME_FMT="${UPTIME_H}h${UPTIME_M}m"
            else
                UPTIME_FMT="${UPTIME_M}m"
            fi
        fi
    fi

    TTL_SECS=0
    if echo "$TTL" | grep -qE '^[0-9]+m[0-9]+s$'; then
        TTL_MIN=$(echo "$TTL" | sed 's/m.*//')
        TTL_SEC_PART=$(echo "$TTL" | sed 's/.*m//;s/s//')
        TTL_SECS=$(( TTL_MIN * 60 + TTL_SEC_PART ))
    fi

    SIGNAL_LINE=$(tail -200 "$LOG" 2>/dev/null | grep -E '\[IDLE\]|\[SKIP\]' | tail -1 || true)
    SIGNAL_TIME=$(logtime "$SIGNAL_LINE")
    SIGNAL_MSG=$(echo "$SIGNAL_LINE" | sed 's/.*\[\(IDLE\|SKIP\)\] *//' | cut -c1-50)

    MKT_YES=$(echo "$SIGNAL_LINE" | sed -n 's/.*Yes=\([0-9.]*\).*/\1/p')
    MKT_NO=$(echo "$SIGNAL_LINE" | sed -n 's/.*No=\([0-9.]*\).*/\1/p')

    CONSEC_LOSSES=0; PAUSE_UNTIL=0
    if [ -f "$STATE" ]; then
        CONSEC_LOSSES=$(sed -n 's/.*"consecutive_losses":\([0-9]*\).*/\1/p' "$STATE" 2>/dev/null || echo 0)
        PAUSE_UNTIL=$(sed -n 's/.*"pause_until_ms":\([0-9]*\).*/\1/p' "$STATE" 2>/dev/null || echo 0)
    fi
    CONSEC_LOSSES="${CONSEC_LOSSES:-0}"
    PAUSE_UNTIL="${PAUSE_UNTIL:-0}"
    NOW_EPOCH_S=$(date "+%s")

    TRADES_CSV="${ROOT}/logs/${MODE}/trades.csv"
    TRADES_TODAY=0
    if [ -f "$TRADES_CSV" ]; then
        TRADES_TODAY=$(grep -cE '^[0-9]+:[0-9]+:[0-9]+,(UP|DOWN),' "$TRADES_CSV" 2>/dev/null || echo 0)
    fi

    BOT_PID=$(pgrep -f 'polybot' 2>/dev/null | head -1 || true)

    # ── format values ──

    if [ "$MODE" = "live" ]; then
        MODE_LABEL="${R}LIVE${N}"
    else
        MODE_LABEL="${D}PAPER${N}"
    fi

    if [ -n "$PNL" ]; then
        case "$PNL" in
            +*) PNL_FMT="${G}${PNL}${N}" ;;
            -*)  PNL_FMT="${R}${PNL}${N}" ;;
            *)   PNL_FMT="$PNL" ;;
        esac
    else
        PNL_FMT="${D}0.00${N}"
    fi

    if [ -n "$BTC" ] && [ "$BTC" != "0" ]; then
        BTC_FMT=$(printf "%'d" "${BTC%.*}" 2>/dev/null || echo "$BTC")
    else
        BTC_FMT="${D}waiting${N}"
    fi

    STREAK_FMT="${D}0${N}"
    if [ -n "$STREAK" ]; then
        case "$STREAK" in
            +*) STREAK_FMT="${G}${STREAK}${N}" ;;
            -*) STREAK_FMT="${R}${STREAK}${N}" ;;
            *)  STREAK_FMT="${Y}${STREAK}${N}" ;;
        esac
    fi

    WR_FMT="${D}--${N}"
    if [ -n "$WIN_RATE" ]; then
        WR_INT=${WIN_RATE%.*}
        if [ "$WR_INT" -ge 55 ] 2>/dev/null; then
            WR_FMT="${G}${WIN_RATE}%${N}"
        elif [ "$WR_INT" -ge 45 ] 2>/dev/null; then
            WR_FMT="${Y}${WIN_RATE}%${N}"
        else
            WR_FMT="${R}${WIN_RATE}%${N}"
        fi
    fi

    if [ -n "$BOT_PID" ]; then
        PROC_ICON="${G}*${N}"
    else
        PROC_ICON="${R}*${N}"
    fi

    CB_MAX=8
    CB_FMT="${G}${CONSEC_LOSSES}/${CB_MAX}${N}"
    if [ "$CONSEC_LOSSES" -ge 6 ] 2>/dev/null; then
        CB_FMT="${R}${CONSEC_LOSSES}/${CB_MAX} !!${N}"
    elif [ "$CONSEC_LOSSES" -ge 4 ] 2>/dev/null; then
        CB_FMT="${Y}${CONSEC_LOSSES}/${CB_MAX}${N}"
    fi

    BOT_STATUS="${G}ACTIVE${N}"
    PAUSE_UNTIL_S=$(( PAUSE_UNTIL / 1000 ))
    if [ "$PAUSE_UNTIL_S" -gt "$NOW_EPOCH_S" ] 2>/dev/null; then
        PAUSE_REM=$(( PAUSE_UNTIL_S - NOW_EPOCH_S ))
        BOT_STATUS="${Y}PAUSED ${PAUSE_REM}s${N}"
    elif [ "$CONSEC_LOSSES" -ge 8 ] 2>/dev/null; then
        BOT_STATUS="${R}CIRCUIT BREAK${N}"
    fi

    DL_FMT=""
    if [ -n "$DAILY_PNL_RAW" ] && [ "$BALANCE" != "?" ]; then
        DAILY_LOSS_PCT=$(awk "BEGIN { b=$BALANCE+0; p=$DAILY_PNL_RAW+0; if(b>0 && p<0) printf \"%.1f\",(-p/b)*100; else print \"0.0\" }" 2>/dev/null || echo "0.0")
        DL_INT=${DAILY_LOSS_PCT%.*}
        if [ "$DL_INT" -ge 8 ] 2>/dev/null; then
            DL_FMT="${R}${DAILY_LOSS_PCT}%${N}${D}/10%${N}"
        elif [ "$DL_INT" -ge 5 ] 2>/dev/null; then
            DL_FMT="${Y}${DAILY_LOSS_PCT}%${N}${D}/10%${N}"
        else
            DL_FMT="${G}${DAILY_LOSS_PCT}%${N}${D}/10%${N}"
        fi
    fi

    # ── render ──

    printf ' %s  %s  %s %s  up %s\n' \
        "${B}POLYBOT${N}" "$MODE_LABEL" "$(date +%H:%M:%S)" "$PROC_ICON" "${C}${UPTIME_FMT}${N}"
    echo ""

    printf '  BTC  $%s\n' "$BTC_FMT"
    printf '  MKT  %s\n' "${C}${SLUG:-discovering...}${N}"

    TTL_BAR=$(progress_bar "$((300 - TTL_SECS))" 300 20)
    printf '  TTL  %s  [%s]  %s\n' "${G}${TTL:-?}${N}" "${D}${TTL_BAR}${N}" "${D}ends ${END_SHORT:-?}${N}"

    if [ -n "$MKT_YES" ] && [ -n "$MKT_NO" ]; then
        EDGE_DISPLAY=""
        CHEAP=$(awk "BEGIN { print ($MKT_YES > $MKT_NO) ? $MKT_NO : $MKT_YES }" 2>/dev/null || true)
        if [ -n "$CHEAP" ]; then
            EDGE_DISPLAY=$(awk "BEGIN { e=(0.50-$CHEAP)*100; if(e>0) printf \"%.0f\",e; else print \"0\" }" 2>/dev/null || true)
        fi
        if [ -n "$EDGE_DISPLAY" ] && [ "$EDGE_DISPLAY" -gt 0 ] 2>/dev/null; then
            printf '  BID  Yes %s / No %s  %s\n' "${Y}${MKT_YES}${N}" "${Y}${MKT_NO}${N}" "${M}edge ~${EDGE_DISPLAY}%%${N}"
        else
            printf '  BID  Yes %s / No %s\n' "${Y}${MKT_YES}${N}" "${Y}${MKT_NO}${N}"
        fi
    else
        printf '  BID  %s\n' "${D}no prices${N}"
    fi
    echo ""

    printf '  Balance  %s\n' "${B}\$${BALANCE}${N}"
    printf '  P&L      %s\n' "$PNL_FMT"
    printf '  Record   %s  win %s\n' "${G}${WL:-0W/0L}${N}" "$WR_FMT"
    printf '  Streak   %s\n' "$STREAK_FMT"
    if [ "$PENDING" != "0" ] && [ -n "$PENDING" ]; then
        printf '  Pending  %s\n' "${Y}${PENDING}${N}"
    fi
    printf '  Trades   %s\n' "${C}${TRADES_TODAY}${N}"
    echo ""

    printf '%s\n' "${D}-- risk --${N}"
    printf '  Breaker  %s  Loss  %s  %s\n' "$CB_FMT" "${DL_FMT:-${D}--${N}}" "$BOT_STATUS"
    echo ""

    printf '%s\n' "${D}-- signal --${N}"
    if [ -n "$SIGNAL_MSG" ]; then
        if echo "$SIGNAL_LINE" | grep -q '\[IDLE\]'; then
            printf '  %s  %s\n' "${D}${SIGNAL_TIME}${N}" "${D}${SIGNAL_MSG}${N}"
        else
            printf '  %s  %s\n' "${D}${SIGNAL_TIME}${N}" "${Y}${SIGNAL_MSG}${N}"
        fi
    else
        printf '  %s\n' "${D}waiting...${N}"
    fi
    echo ""

    printf '%s\n' "${D}-- last trade --${N}"
    if [ -n "$TRADE" ]; then
        T_TIME=$(logtime "$TRADE")
        T_DIR=$(echo "$TRADE" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
        T_PRICE=$(echo "$TRADE" | sed -n 's/.*@ *\([0-9.]*\).*/\1/p')
        T_EDGE=$(echo "$TRADE" | sed -n 's/.*edge=\([0-9]*\).*/\1/p')
        T_BTC=$(echo "$TRADE" | sed -n 's/.*BTC=\$\([0-9]*\).*/\1/p')
        if [ "$T_DIR" = "UP" ]; then
            DIR_FMT="${G}UP${N}"
        else
            DIR_FMT="${R}DN${N}"
        fi
        printf '  %s  %s @%s  edge %s' "${D}${T_TIME}${N}" "$DIR_FMT" "${B}${T_PRICE}${N}" "${Y}${T_EDGE}%%${N}"
        [ -n "$T_BTC" ] && printf '  %s' "${D}BTC\$${T_BTC}${N}"
        printf '\n'
    else
        printf '  %s\n' "${D}none${N}"
    fi
    echo ""

    printf '%s\n' "${D}-- activity --${N}"
    ACTIVITY=$(tail -500 "$LOG" 2>/dev/null | grep -E '\[SETTLED\]|\[TRADE\]|\[MKT\].*(found|ends)|\[REDEEM\]|\[RISK\]' | tail -8 || true)
    if [ -n "$ACTIVITY" ]; then
        echo "$ACTIVITY" | while IFS= read -r line; do
            TIME=$(logtime "$line")
            if echo "$line" | grep -q '\[SETTLED\]'; then
                S_PNL=$(echo "$line" | sed -n 's/.*pnl=\([^ ]*\).*/\1/p')
                S_DIR=$(echo "$line" | sed -n 's/.*\(UP\|DOWN\).*/\1/p')
                if echo "$line" | grep -q 'WIN'; then
                    printf '  %s  %s %s %s\n' "${D}${TIME}${N}" "${G}WIN${N}" "${G}${S_PNL}${N}" "${D}${S_DIR}${N}"
                else
                    printf '  %s  %s %s %s\n' "${D}${TIME}${N}" "${R}LOSS${N}" "${R}${S_PNL}${N}" "${D}${S_DIR}${N}"
                fi
            elif echo "$line" | grep -q '\[TRADE\]'; then
                A_DIR=$(echo "$line" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
                A_PRICE=$(echo "$line" | sed -n 's/.*@ *\([0-9.]*\).*/\1/p')
                A_EDGE=$(echo "$line" | sed -n 's/.*edge=\([0-9]*\).*/\1/p')
                printf '  %s  %s %s @%s e%s\n' "${D}${TIME}${N}" "${C}BUY${N}" "$A_DIR" "$A_PRICE" "${D}${A_EDGE}%%${N}"
            elif echo "$line" | grep -q '\[MKT\]'; then
                M_SLUG=$(echo "$line" | sed -n 's/.*\(5m-[0-9]*\).*/\1/p')
                printf '  %s  %s %s\n' "${D}${TIME}${N}" "${D}MKT${N}" "${D}${M_SLUG}${N}"
            elif echo "$line" | grep -q '\[REDEEM\]'; then
                R_TX=$(echo "$line" | sed -n 's/.*tx=\([^ ]*\).*/\1/p' | cut -c1-10)
                printf '  %s  %s %s\n' "${D}${TIME}${N}" "${M}REDEEM${N}" "${D}${R_TX}${N}"
            elif echo "$line" | grep -q '\[RISK\]'; then
                R_MSG=$(echo "$line" | sed 's/.*\[RISK\] //' | cut -c1-40)
                printf '  %s  %s\n' "${D}${TIME}${N}" "${Y}${R_MSG}${N}"
            fi
        done
    else
        printf '  %s\n' "${D}none${N}"
    fi
    echo ""

    ERRORS=$(tail -500 "$LOG" 2>/dev/null | grep -E '\[EXEC\].*failed|\[EXEC\].*FOK|\[BAL\].*[Ff]ailed|\[PRICE\].*stale' | tail -3 || true)
    if [ -n "$ERRORS" ]; then
        printf '%s\n' "${D}-- errors --${N}"
        echo "$ERRORS" | while IFS= read -r line; do
            E_TIME=$(logtime "$line")
            if echo "$line" | grep -q 'FOK'; then
                E_PRICE=$(echo "$line" | sed -n 's/.*at \([0-9.]*\).*/\1/p')
                printf '  %s  %s @%s\n' "${D}${E_TIME}${N}" "${Y}FOK rejected${N}" "$E_PRICE"
            elif echo "$line" | grep -q 'stale'; then
                E_AGE=$(echo "$line" | sed -n 's/.*(\([0-9]*s\)).*/\1/p')
                printf '  %s  %s %s\n' "${D}${E_TIME}${N}" "${Y}stale${N}" "${D}${E_AGE}${N}"
            else
                E_MSG=$(echo "$line" | sed 's/.*order failed: //' | cut -c1-40)
                printf '  %s  %s\n' "${D}${E_TIME}${N}" "${R}${E_MSG}${N}"
            fi
        done
        echo ""
    fi

    LOG_SIZE=$(du -h "$LOG" 2>/dev/null | awk '{print $1}' || echo "?")
    printf '%s\n' "${D}${LOG_SIZE} | ${SEC}s | q${N}"

    tput ed; sleep "$SEC"
done
