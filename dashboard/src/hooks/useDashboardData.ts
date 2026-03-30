import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import type { TradeRecord } from '@/lib/dashboard-types'
import { parseTrades } from '@/lib/csv-parser'
import { computeStats } from '@/lib/stats'

const REFRESH_INTERVAL_MS = 30_000

interface DashboardDataState {
  trades: TradeRecord[]
  balance: number
  loading: boolean
  error: string | null
  lastUpdated: Date | null
}

async function fetchBalanceFile(signal?: AbortSignal): Promise<number | null> {
  try {
    const response = await fetch('balance', { cache: 'no-store', signal })
    if (!response.ok) return null

    const rawBalance = (await response.text()).trim()
    const parsed = Number.parseFloat(rawBalance)
    return Number.isFinite(parsed) ? parsed : null
  } catch {
    return null
  }
}

function balanceFromTrades(trades: TradeRecord[]): number {
  // Find the last entry that recorded a balance, then replay settlements after it.
  //
  // Accounting model:
  //   entry.balance  = balance AFTER cost deduction  (bot does balance -= cost)
  //   settlement adds payout to balance              (bot does balance += payout)
  //   pnl = payout - cost, so payout = pnl + cost
  //
  // For LOSS: payout = 0, balance stays at entry.balance
  // For WIN:  payout = pnl + cost, balance = entry.balance + payout
  let lastEntryBalance: number | null = null
  let lastEntryCost: number = 0
  let lastEntryIndex = -1

  for (let i = trades.length - 1; i >= 0; i -= 1) {
    const t = trades[i]
    if (t.type === 'entry' && t.balance != null) {
      lastEntryBalance = t.balance
      lastEntryCost = t.cost ?? 0
      lastEntryIndex = i
      break
    }
  }

  if (lastEntryBalance == null) return 0

  // Apply payouts (not PnL) from settlements after the last entry
  let totalPayout = 0
  for (let i = lastEntryIndex + 1; i < trades.length; i += 1) {
    const t = trades[i]
    if (t.type === 'settlement') {
      // payout = pnl + cost; for LOSS pnl=-cost so payout=0
      const payout = Math.max(0, t.pnl + lastEntryCost)
      totalPayout += payout
    }
  }

  return lastEntryBalance + totalPayout
}

export function useDashboardData() {
  const [state, setState] = useState<DashboardDataState>({
    trades: [],
    balance: 0,
    loading: true,
    error: null,
    lastUpdated: null,
  })

  const abortRef = useRef<AbortController | null>(null)

  const loadData = useCallback(async () => {
    // Cancel any in-flight request
    abortRef.current?.abort()
    const controller = new AbortController()
    abortRef.current = controller

    try {
      const response = await fetch('trades.csv', { cache: 'no-store', signal: controller.signal })

      if (!response.ok) {
        throw new Error(`Failed to fetch trades.csv (HTTP ${response.status})`)
      }

      const csv = await response.text()
      const parsedTrades = parseTrades(csv)

      const fileBalance = await fetchBalanceFile(controller.signal)
      const derivedBalance = balanceFromTrades(parsedTrades)
      // Prefer balance file, but fall back to CSV-derived balance when file is
      // missing or zero (zero usually means the bot restarted and lost state)
      const parsedBalance = fileBalance && fileBalance > 0 ? fileBalance : derivedBalance

      if (!controller.signal.aborted) {
        setState({
          trades: parsedTrades,
          balance: parsedBalance,
          loading: false,
          error: null,
          lastUpdated: new Date(),
        })
      }
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return
      setState((previous) => ({
        ...previous,
        loading: false,
        error: error instanceof Error ? error.message : 'Unknown error while loading dashboard data',
      }))
    }
  }, [])

  useEffect(() => {
    void loadData()

    const interval = window.setInterval(() => {
      void loadData()
    }, REFRESH_INTERVAL_MS)

    return () => {
      window.clearInterval(interval)
      abortRef.current?.abort()
    }
  }, [loadData])

  const stats = useMemo(() => computeStats(state.trades), [state.trades])

  return {
    trades: state.trades,
    stats,
    balance: state.balance,
    loading: state.loading,
    error: state.error,
    lastUpdated: state.lastUpdated,
    refresh: loadData,
  }
}
