import { AlertTriangle, LoaderCircle } from 'lucide-react'

import { DirectionChart } from '@/components/DirectionChart'
import { EquityChart } from '@/components/EquityChart'
import { Header } from '@/components/Header'
import { HourlyChart } from '@/components/HourlyChart'
import { PerformanceCard } from '@/components/PerformanceCard'
import { StatsCards } from '@/components/StatsCards'
import { TradesTable } from '@/components/TradesTable'
import { Button } from '@/components/ui/button'
import { useDashboardData } from '@/hooks/useDashboardData'

export function Dashboard() {
  const { trades, stats, balance, loading, error, lastUpdated, refresh } = useDashboardData()

  if (loading) {
    return (
      <div className="flex min-h-screen flex-col items-center justify-center gap-4">
        <LoaderCircle className="size-10 animate-spin text-[var(--accent)]" />
        <span className="text-sm text-[var(--text-secondary)]">Loading trades...</span>
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex min-h-screen flex-col items-center justify-center gap-4 px-6 text-center">
        <AlertTriangle className="size-10 text-[var(--loss)]" />
        <div className="text-base text-[var(--loss)]">Failed to load data</div>
        <div className="mono max-w-xl text-xs text-[var(--text-dim)]">{error}</div>
        <div className="text-xs text-[var(--text-dim)]">
          Make sure this app is served from the oracel/logs/live/ or logs/paper/ directory.
        </div>
        <Button
          variant="outline"
          className="border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-primary)] hover:bg-[var(--accent-dim)]"
          onClick={() => {
            void refresh()
          }}
        >
          Retry
        </Button>
      </div>
    )
  }

  return (
    <div className="min-h-screen text-[var(--text-primary)]">
      <Header btcPrice={stats.lastBTC} lastUpdated={lastUpdated} />

      <main className="mx-auto w-full max-w-7xl space-y-5 px-3 py-4 sm:space-y-6 sm:px-6 sm:py-6">
        <StatsCards stats={stats} balance={balance} lineCount={trades.length} />

        <section className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <EquityChart equity={stats.equity} />
          <PerformanceCard stats={stats} />
        </section>

        <section className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <DirectionChart data={stats.directionStats} />
          <HourlyChart data={stats.hourlySeries} />
        </section>

        <TradesTable trades={trades} pendingTrades={stats.pending} />
      </main>
    </div>
  )
}
