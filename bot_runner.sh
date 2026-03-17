#!/bin/bash
while true; do
  ./target/release/polybot > bot.log 2>&1
  sleep 5
done
