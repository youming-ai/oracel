import { useMemo } from 'react'
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { EquityPoint } from '@/lib/dashboard-types'
import { formatCurrency } from '@/lib/format'
import { TrendingUp } from 'lucide-react'

interface EquityChartProps {
  equity: EquityPoint[]
  balance: number
  totalPnl: number
}

export function EquityChart({ equity, balance, totalPnl }: EquityChartProps) {
  const chartData = useMemo(() => {
    // Derive starting balance: current balance minus total PnL
    const startingBalance = balance - totalPnl
    return equity.map((point, index) => ({
      ...point,
      balance: startingBalance + point.cumulativePnl,
      label: index + 1,
    }))
  }, [equity, balance, totalPnl])

  return (
    <Card className="hud-card gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-4 pt-4 pb-3 sm:px-5 sm:pt-5 sm:pb-4">
        <CardTitle className="card-title-hud">
          <TrendingUp className="size-3.5 text-[var(--accent)]" />
          Equity Curve
        </CardTitle>
      </CardHeader>

      <CardContent className="h-[200px] px-2 pb-3 sm:h-[240px] sm:px-4 sm:pb-5">
        {chartData.length === 0 ? (
          <div className="mono flex h-full items-center justify-center text-sm text-[var(--text-dim)]">
            No settled trades yet
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={chartData} margin={{ top: 10, right: 4, left: -14, bottom: 0 }}>
              <defs>
                <linearGradient id="equityFill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="rgba(0,212,170,0.2)" />
                  <stop offset="100%" stopColor="rgba(0,212,170,0)" />
                </linearGradient>
              </defs>

              <CartesianGrid stroke="rgba(30,45,61,0.3)" strokeDasharray="0" vertical={false} />
              <XAxis dataKey="label" hide />
              <YAxis
                tick={{ fill: 'var(--text-secondary)', fontFamily: '"Geist Pixel", monospace', fontSize: 10 }}
                tickFormatter={(value) => formatCurrency(value as number, 0)}
                axisLine={false}
                tickLine={false}
              />
              <Tooltip
                cursor={{ stroke: 'rgba(0,212,170,0.35)', strokeWidth: 1 }}
                formatter={(value) => formatCurrency(Number(value), 2)}
                contentStyle={{
                  background: 'rgba(10,14,23,0.95)',
                  border: '1px solid rgba(0,212,170,0.2)',
                  borderRadius: '6px',
                  color: 'var(--text-primary)',
                  fontFamily: '"Geist Pixel", monospace',
                  fontSize: '11px',
                  boxShadow: '0 4px 12px rgba(0,0,0,0.4)',
                }}
                labelStyle={{ color: 'var(--text-secondary)' }}
              />
              <Area
                type="monotone"
                dataKey="balance"
                stroke="var(--accent)"
                strokeWidth={1.5}
                fill="url(#equityFill)"
                dot={false}
                isAnimationActive={false}
              />
            </AreaChart>
          </ResponsiveContainer>
        )}
      </CardContent>
    </Card>
  )
}
