#!/bin/bash
# Real-time log beautifier for polybot
# Usage: ./watch.sh [log_file]

LOG="${1:-/tmp/polybot.log}"
REFRESH="${2:-5}"

# Colors
RED='\033[1;31m'
GREEN='\033[1;32m'
YELLOW='\033[1;33m'
BLUE='\033[1;34m'
CYAN='\033[1;36m'
DIM='\033[2m'
BOLD='\033[1m'
NC='\033[0m'

# Clear screen
clear

while true; do
    # Move cursor to top
    tput cup 0 0
    
    # Header
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║  🤖 Polymarket 5m Bot — Live Monitor                                ║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════╣${NC}"
    
    # Stats
    TOTAL_SIGNALS=$(grep -c '\[SIGNAL\]' "$LOG" 2>/dev/null || echo 0)
    TOTAL_TRADES=$(grep -c 'TRADE:' "$LOG" 2>/dev/null || echo 0)
    TOTAL_NO_TRADE=$(grep -c 'NO TRADE' "$LOG" 2>/dev/null || echo 0)
    ROTATIONS=$(grep -c 'Market updated:' "$LOG" 2>/dev/null || echo 0)
    WARNINGS=$(grep -c 'WARN' "$LOG" 2>/dev/null || echo 0)
    LAST_PRICE=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | tail -1 | grep -oP 'Bin=\$\K[0-9]+' || echo "?")
    LAST_ORACLE=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | tail -1 | grep -oP 'Oracle=\$\K[0-9]+' || echo "?")
    LAST_EDGE=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | tail -1 | grep -oP 'Edge: \K[0-9.]+' || echo "?")
    LAST_MKT_YES=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | tail -1 | grep -oP 'Yes=\K[0-9.]+' || echo "?")
    REGIME=$(grep '\[SIGNAL\]' "$LOG" 2>/dev/null | tail -1 | grep -oP 'Regime=\K[A-Za-z]+' || echo "?")
    
    printf "║  ${CYAN}Signals:${NC} %-6s ${GREEN}Trades:${NC} %-5s ${YELLOW}Skip:${NC} %-5s ${BLUE}Rotations:${NC} %-3s ${RED}Warnings:${NC} %-3s ║\n" \
           "$TOTAL_SIGNALS" "$TOTAL_TRADES" "$TOTAL_NO_TRADE" "$ROTATIONS" "$WARNINGS"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════╣${NC}"
    
    printf "║  ${BOLD}BTC:${NC} ${GREEN}\$%-8s${NC}  Oracle: ${CYAN}\$%-8s${NC}  Edge: ${YELLOW}%-6s%%${NC}  Yes: %-5s  %-8s ║\n" \
           "$LAST_PRICE" "$LAST_ORACLE" "$LAST_EDGE" "$LAST_MKT_YES" "$REGIME"
    
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════╣${NC}"
    echo -e "${BOLD}║  Recent Activity                                                     ║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════╣${NC}"
    
    # Get last 12 signal/trade lines, stripped of ANSI
    tail -100 "$LOG" 2>/dev/null | \
        sed 's/\x1b\[[0-9;]*m//g' | \
        grep -E '\[SIGNAL\]|TRADE:|PAPER|Market (discovered|updated)|Chainlink pol' | \
        tail -12 | \
        while IFS= read -r line; do
            # Extract time
            TIME=$(echo "$line" | grep -oP '^\S+T\K[0-9:]+' | cut -c1-8)
            
            if echo "$line" | grep -q 'TRADE:'; then
                SIDE=$(echo "$line" | grep -oP 'TRADE: \K\w+')
                EDGE=$(echo "$line" | grep -oP 'edge: \K[0-9.]+')
                PRICE=$(echo "$line" | grep -oP '@ \K[0-9.]+')
                printf "║  ${DIM}%s${NC}  ${GREEN}▶ TRADE ${BOLD}%s${NC} @ ${YELLOW}%.3f${NC}  edge=${GREEN}%.1f%%${NC}\n" "$TIME" "$SIDE" "$PRICE" "$EDGE"
            elif echo "$line" | grep -q 'NO TRADE'; then
                REASON=$(echo "$line" | grep -oP 'NO TRADE \(\K[^)]+')
                YES=$(echo "$line" | grep -oP 'Yes=\K[0-9.]+')
                NO=$(echo "$line" | grep -oP 'No=\K[0-9.]+')
                printf "║  ${DIM}%s${NC}  ${YELLOW}■ SKIP  ${NC}  Yes=%.3f No=%.3f  ${DIM}%s${NC}\n" "$TIME" "$YES" "$NO" "$REASON"
            elif echo "$line" | grep -q 'Market updated:'; then
                SLUG=$(echo "$line" | grep -oP 'updated: \K\S+')
                printf "║  ${DIM}%s${NC}  ${BLUE}↻ Market → %s${NC}\n" "$TIME" "$SLUG"
            elif echo "$line" | grep -q 'Market discovered:'; then
                SLUG=$(echo "$line" | grep -oP 'discovered: \K\S+')
                printf "║  ${DIM}%s${NC}  ${BLUE}● Market: %s${NC}\n" "$TIME" "$SLUG"
            elif echo "$line" | grep -q 'Chainlink'; then
                printf "║  ${DIM}%s${NC}  ${RED}⚠ Chainlink${NC}\n" "$TIME"
            fi
        done
    
    # Fill remaining lines
    LINES_SHOWN=$(tail -100 "$LOG" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | grep -c -E '\[SIGNAL\]|TRADE:|PAPER|Market (discovered|updated)|Chainlink pol')
    for ((i=LINES_SHOWN; i<12; i++)); do
        echo "║                                                                       ║"
    done
    
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════╣${NC}"
    
    # Edge distribution
    echo -e "${BOLD}║  Edge Distribution (last 20 trades)                                  ║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════════╣${NC}"
    
    grep '\[SIGNAL\]' "$LOG" 2>/dev/null | tail -20 | grep -oP 'Edge: \K[0-9.]+' | \
        awk '{
            if ($1 >= 50) b70++
            else if ($1 >= 30) b50++
            else if ($1 >= 10) b30++
            else b10++
            sum+=$1; n++
        }
        END {
            if (n==0) { print "║  No data yet                                                        ║"; exit }
            avg=sum/n
            printf "║  Avg: %.1f%%  ", avg
            if (b70>0) printf "█ 50%%+: %d ", b70
            if (b50>0) printf "▓ 30-50: %d ", b50
            if (b30>0) printf "▒ 10-30: %d ", b30
            if (b10>0) printf "░ <10: %d ", b10
            printf "         ║\n"
        }')
    
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════════╝${NC}"
    echo -e "${DIM}  Refreshing every ${REFRESH}s | Ctrl+C to exit | Log: $LOG${NC}"
    
    sleep "$REFRESH"
    tput ed  # Clear to end of screen
done
