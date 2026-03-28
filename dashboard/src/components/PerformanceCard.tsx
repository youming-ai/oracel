import { Cell, Pie, PieChart, ResponsiveContainer } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { DashboardStats } from '@/lib/dashboard-types'
import { formatCurrency, formatPercent } from '@/lib/format'

interface PerformanceCardProps {
  stats: DashboardStats
}

function getWinRateColor(winRate: number): string {
  if (winRate >= 15) {
    return 'var(--win)'
  }

  if (winRate >= 10) {
    return 'var(--warn)'
  }

  return 'var(--loss)'
}

export function PerformanceCard({ stats }: PerformanceCardProps) {
  const totalTrades = stats.settlements.length
  const winRateColor = getWinRateColor(stats.winRate)

  return (
    <Card className="glass gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-5 pt-5 pb-4">
        <CardTitle className="text-sm font-semibold text-[var(--text-secondary)]">Performance</CardTitle>
      </CardHeader>

      <CardContent className="px-5 pb-5">
        <div className="flex flex-col gap-6 lg:flex-row lg:items-center lg:gap-8">
          <div className="relative mx-auto h-40 w-40 shrink-0 lg:mx-0">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  data={[
                    { name: 'Wins', value: stats.wins.length },
                    { name: 'Losses', value: stats.losses.length },
                  ]}
                  dataKey="value"
                  innerRadius={48}
                  outerRadius={78}
                  stroke="none"
                  isAnimationActive={false}
                >
                  <Cell fill="var(--win)" />
                  <Cell fill="var(--loss)" />
                </Pie>
              </PieChart>
            </ResponsiveContainer>

            <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
              <div className="mono text-2xl font-bold" style={{ color: winRateColor }}>
                {formatPercent(stats.winRate)}
              </div>
              <div className="text-xs text-[var(--text-dim)]">Win Rate</div>
            </div>
          </div>

          <div className="w-full space-y-3">
            <Metric label="Total Trades" value={`${totalTrades}`} />
            <Metric label="Wins" value={`${stats.wins.length}`} labelColor="var(--win)" valueColor="var(--win)" />
            <Metric
              label="Losses"
              value={`${stats.losses.length}`}
              labelColor="var(--loss)"
              valueColor="var(--loss)"
            />
            <Metric label="Avg Win" value={formatCurrency(stats.avgWin)} valueColor="var(--win)" />
            <Metric label="Avg Loss" value={`-${formatCurrency(stats.avgLoss)}`} valueColor="var(--loss)" />
            <Metric
              label="Profit Factor"
              value={Number.isFinite(stats.profitFactor) ? stats.profitFactor.toFixed(2) : '∞'}
              valueColor={
                Number.isFinite(stats.profitFactor) && stats.profitFactor > 1 ? 'var(--win)' : 'var(--loss)'
              }
            />
            <Metric label="Max Drawdown" value={`-${formatCurrency(stats.maxDrawdown)}`} valueColor="var(--loss)" />
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

interface MetricProps {
  label: string
  value: string
  labelColor?: string
  valueColor?: string
}

function Metric({ label, value, labelColor, valueColor }: MetricProps) {
  return (
    <div className="flex items-center justify-between text-xs">
      <span style={{ color: labelColor ?? 'var(--text-secondary)' }}>{label}</span>
      <span className="mono font-semibold" style={{ color: valueColor ?? 'var(--text-primary)' }}>
        {value}
      </span>
    </div>
  )
}
