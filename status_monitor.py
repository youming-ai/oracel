#!/usr/bin/env python3
import os
import subprocess
import sys
import json
import time
from datetime import datetime, timedelta

def report_status():
    """Report bot status"""
    print(f"=== {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} Bot Status ===")
    
    # Check if bot is running
    ps_result = subprocess.run(['ps', 'aux', '|', 'grep', 'polybot'], 
                              capture_output=True, text=True, shell=True)
    if "polybot" in ps_result.stdout and "grep" not in ps_result.stdout:
        print("Bot: Running")
    else:
        print("Bot: Not running")
        return False
    
    # Check trade log
    trade_log_path = "trade_log.json"
    if os.path.exists(trade_log_path):
        with open(trade_log_path, 'r') as f:
            lines = f.readlines()
            if lines:
                print(f"Total trades: {len(lines)}")
                # Parse last few trades
                recent_trades = lines[-10:]
                for trade in recent_trades:
                    try:
                        trade_data = json.loads(trade.strip())
                        print(f"  {trade_data['timestamp']} - {trade_data['side']} ${trade_data['size']} @ ${trade_data['price']}")
                    except:
                        continue
    else:
        print("Trade log: Not found")
    
    # Check balance (if available)
    # This would require parsing the trade log to calculate current balance
    return True

if __name__ == "__main__":
    # Initial report
    print("Bot monitoring started. Will report status every hour.")
    
    # Schedule hourly reports
    next_report_time = datetime.now().replace(minute=0, second=0, microsecond=0) + timedelta(hours=1)
    while True:
        now = datetime.now()
        if now >= next_report_time:
            print(f"\n{'='*50}")
            report_status()
            print(f"{'='*50}\n")
            next_report_time = next_report_time + timedelta(hours=1)
        
        time.sleep(60)  # Check every minute