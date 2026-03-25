#!/bin/bash
# Usage: scripts/watch.sh [mode] [refresh]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="${1:-paper}"
SEC="${2:-3}"

LOG_DIR="${ROOT}/logs/${MODE}"
STATE="${LOG_DIR}/state.json"

LOG=$(ls -t "${LOG_DIR}"/bot.log* 2>/dev/null | head -1)
[ -z "${LOG:-}" ] && LOG="${LOG_DIR}/bot.log"

G=$'\033[32m'; R=$'\033[31m'; Y=$'\033[33m'; C=$'\033[36m'
B=$'\033[1m'; D=$'\033[2m'; N=$'\033[0m'

line() { printf "%-${COLUMNS:-80}s\n" "$*"; }

color_pnl() { case "$1" in +*) printf "${G}%s${N}" "$1";; -*) printf "${R}%s${N}" "$1";; *) printf "%s" "$1";; esac; }
color_streak() { case "$1" in +*) printf "${G}%s${N}" "$1";; -*) printf "${R}%s${N}" "$1";; *) printf "${D}%s${N}" "${1:-0}";; esac; }

tput smcup 2>/dev/null; tput civis 2>/dev/null
trap 'tput cnorm 2>/dev/null; tput rmcup 2>/dev/null' EXIT
trap 'exit' INT TERM

while true; do
    # ── collect data (single tail + one awk pass) ──
    eval "$(tail -300 "$LOG" 2>/dev/null | awk '
        /\[STATUS\]/  { status = $0 }
        /\[IDLE\]|\[SKIP\]/ { signal = $0 }
        /\[TRADE\]/   { trade = $0 }
        /\[SETTLED\]|\[TRADE\]|\[RISK\]/ {
            if ($0 != last_act) {
                act[++ai] = $0
                last_act = $0
                if (ai > 5) { for (i=1;i<5;i++) act[i]=act[i+1]; ai=5 }
            }
        }
        END {
            gsub(/'\''/, "'\''\\'\'''\''", status)
            gsub(/'\''/, "'\''\\'\'''\''", signal)
            gsub(/'\''/, "'\''\\'\'''\''", trade)
            printf "STATUS='\''%s'\''\n", status
            printf "SIGNAL='\''%s'\''\n", signal
            printf "TRADE='\''%s'\''\n", trade
            printf "ACT_N=%d\n", ai
            for (i=1;i<=ai;i++) {
                gsub(/'\''/, "'\''\\'\'''\''", act[i])
                printf "ACT_%d='\''%s'\''\n", i, act[i]
            }
        }
    ')"

    BAL=$(cat "${ROOT}/logs/${MODE}/balance" 2>/dev/null || echo "?")

    # parse STATUS line
    BTC=$(echo "$STATUS" | sed -n 's/.*BTC=\$\([^ |]*\).*/\1/p')
    PNL=$(echo "$STATUS" | sed -n 's/.*pnl=\([^ |]*\).*/\1/p')
    TTL=$(echo "$STATUS" | sed -n 's/.*ttl=\([^ |]*\).*/\1/p')
    WL=$(echo "$STATUS" | sed -n 's/.*| \([0-9]*W\/[0-9]*L\).*/\1/p')
    STREAK=$(echo "$STATUS" | sed -n 's/.*streak=\([^ |]*\).*/\1/p')
    PENDING=$(echo "$STATUS" | sed -n 's/.*pending=\([^ |]*\).*/\1/p')

    # parse state.json
    CL=0
    if [ -f "$STATE" ]; then
        CL=$(sed -n 's/.*"consecutive_losses":\([0-9]*\).*/\1/p' "$STATE" 2>/dev/null || echo 0)
    fi

    # parse signal
    SIG_MSG=$(echo "$SIGNAL" | sed 's/.*\[\(IDLE\|SKIP\)\] *//' | cut -c1-50)
    MKT_YES=$(echo "$SIGNAL" | sed -n 's/.*Yes=\([0-9.]*\).*/\1/p')
    MKT_NO=$(echo "$SIGNAL" | sed -n 's/.*No=\([0-9.]*\).*/\1/p')

    # win rate
    W=$(echo "$WL" | sed -n 's/\([0-9]*\)W.*/\1/p'); W=${W:-0}
    L=$(echo "$WL" | sed -n 's/.*\/\([0-9]*\)L/\1/p'); L=${L:-0}
    TOT=$((W + L))
    WR="--"
    [ "$TOT" -gt 0 ] && WR=$(awk "BEGIN{printf \"%.0f%%\",($W/$TOT)*100}")

    # alive?
    ALIVE="${R}OFF${N}"
    pgrep -qf 'polybot' 2>/dev/null && ALIVE="${G}ON${N}"

    # bot status
    BOT_ST="${G}ACTIVE${N}"
    if [ "${CL:-0}" -ge 8 ] 2>/dev/null; then
        BOT_ST="${R}CIRCUIT${N}"
    fi

    # ── render ──
    tput cup 0 0

    ML="${R}LIVE${N}"; [ "$MODE" = "paper" ] && ML="${D}PAPER${N}"
    line "$(printf " ${B}POLYBOT${N}  %s  %s  %s" "$ML" "$ALIVE" "$(date +%H:%M:%S)")"
    line ""

    # market
    BTC_F="${D}waiting${N}"
    [ -n "$BTC" ] && [ "$BTC" != "0" ] && BTC_F=$(printf "%'d" "${BTC%.*}" 2>/dev/null || echo "$BTC")
    line "$(printf "  BTC   ${B}\$%s${N}     TTL  ${C}%s${N}" "$BTC_F" "${TTL:-?}")"

    if [ -n "$MKT_YES" ] && [ -n "$MKT_NO" ]; then
        line "$(printf "  MKT   Y ${Y}%s${N}  N ${Y}%s${N}" "$MKT_YES" "$MKT_NO")"
    else
        line "$(printf "  MKT   ${D}no prices${N}")"
    fi
    line ""

    # account
    line "$(printf "  BAL   ${B}\$%s${N}     PNL  %s" "$BAL" "$(color_pnl "${PNL:-0.00}")")"
    line "$(printf "  W/L   ${G}%s${N}  %s   STK  %s" "${WL:-0W/0L}" "$WR" "$(color_streak "$STREAK")")"

    # risk
    CB_C="$G"; [ "${CL:-0}" -ge 4 ] 2>/dev/null && CB_C="$Y"; [ "${CL:-0}" -ge 6 ] 2>/dev/null && CB_C="$R"
    line "$(printf "  RISK  ${CB_C}%s/10${N} losses  %s  pend ${C}%s${N}" "${CL:-0}" "$BOT_ST" "${PENDING:-0}")"
    line ""

    # signal
    line "$(printf "  ${D}SIG${N}   ${D}%s${N}" "${SIG_MSG:-waiting...}")"
    line ""

    # last trade
    if [ -n "$TRADE" ]; then
        T_TIME=$(echo "$TRADE" | sed -n 's/^[0-9-]*T\([0-9:]*\)\..*/\1/p')
        T_DIR=$(echo "$TRADE" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
        T_PRICE=$(echo "$TRADE" | sed -n 's/.*@ *\([0-9.]*\).*/\1/p')
        T_EDGE=$(echo "$TRADE" | sed -n 's/.*edge=\([0-9]*\).*/\1/p')
        DC="${G}"; [ "$T_DIR" = "DOWN" ] && DC="$R"
        line "$(printf "  LAST  ${D}%s${N}  ${DC}%s${N} @${B}%s${N}  e${Y}%s%%${N}" "$T_TIME" "$T_DIR" "$T_PRICE" "$T_EDGE")"
    else
        line "$(printf "  LAST  ${D}none${N}")"
    fi
    line ""

    # activity (last 5 events)
    line "$(printf "  ${D}--- activity ---${N}")"
    if [ "${ACT_N:-0}" -gt 0 ] 2>/dev/null; then
        i=1
        while [ "$i" -le "$ACT_N" ]; do
            eval "l=\$ACT_$i"
            T=$(echo "$l" | sed -n 's/^[0-9-]*T\([0-9:]*\)\..*/\1/p')
            case "$l" in
                *'[SETTLED]'*)
                    P=$(echo "$l" | sed -n 's/.*pnl=\([^ ]*\).*/\1/p')
                    case "$l" in
                        *WIN*) line "$(printf "  ${D}%s${N}  ${G}WIN  %s${N}" "$T" "$P")" ;;
                        *)     line "$(printf "  ${D}%s${N}  ${R}LOSS %s${N}" "$T" "$P")" ;;
                    esac ;;
                *'[TRADE]'*)
                    DR=$(echo "$l" | sed -n 's/.*\[TRADE\] *\([A-Z]*\).*/\1/p')
                    PR=$(echo "$l" | sed -n 's/.*@ *\([0-9.]*\).*/\1/p')
                    line "$(printf "  ${D}%s${N}  ${C}BUY${N}  %s @%s" "$T" "$DR" "$PR")" ;;
                *'[RISK]'*)
                    RM=$(echo "$l" | sed 's/.*\[RISK\] //' | cut -c1-40)
                    line "$(printf "  ${D}%s${N}  ${Y}%s${N}" "$T" "$RM")" ;;
            esac
            i=$((i + 1))
        done
    else
        line "$(printf "  ${D}none${N}")"
    fi
    line ""

    LOG_SZ=$(du -h "$LOG" 2>/dev/null | awk '{print $1}' || echo "?")
    line "$(printf "  ${D}%s | %ss | ctrl-c to exit${N}" "$LOG_SZ" "$SEC")"

    tput ed 2>/dev/null
    sleep "$SEC"
done
