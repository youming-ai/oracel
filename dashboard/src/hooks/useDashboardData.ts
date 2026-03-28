import { useCallback, useEffect, useMemo, useState } from 'react'

import type { DashboardStats, TradeRecord } from '@/lib/dashboard-types'
import { parseTrades } from '@/lib/csv-parser'
import { computeStats } from '@/lib/stats'

const REFRESH_INTERVAL_MS = 30_000
const DEFAULT_STARTING_BALANCE = 100

interface DashboardDataState {
  trades: TradeRecord[]
  balance: number
  loading: boolean
  error: string | null
  lastUpdated: Date | null
}

async function fetchBalanceFile(): Promise<number | null> {
  try {
    const response = await fetch('balance', { cache: 'no-store' })
    if (!response.ok) return null

    const rawBalance = (await response.text()).trim()
    const parsed = Number.parseFloat(rawBalance)
    return Number.isFinite(parsed) ? parsed : null
  } catch {
    return null
  }
}

function balanceFromEquity(stats: DashboardStats): number {
  const lastPoint = stats.equity.length > 0 ? stats.equity[stats.equity.length - 1] : null
  return lastPoint ? DEFAULT_STARTING_BALANCE + lastPoint.cumulativePnl : 0
}

export function useDashboardData() {
  const [state, setState] = useState<DashboardDataState>({
    trades: [],
    balance: 0,
    loading: true,
    error: null,
    lastUpdated: null,
  })

  const loadData = useCallback(async () => {
    try {
      const response = await fetch('trades.csv', { cache: 'no-store' })

      if (!response.ok) {
        throw new Error(`Failed to fetch trades.csv (HTTP ${response.status})`)
      }

      const csv = await response.text()
      const parsedTrades = parseTrades(csv)

      const stats = computeStats(parsedTrades)
      const fileBalance = await fetchBalanceFile()
      const parsedBalance = fileBalance ?? balanceFromEquity(stats)

      setState({
        trades: parsedTrades,
        balance: parsedBalance,
        loading: false,
        error: null,
        lastUpdated: new Date(),
      })
    } catch (error) {
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
