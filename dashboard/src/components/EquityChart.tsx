import { useMemo } from 'react'
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { EquityPoint } from '@/lib/dashboard-types'
import { formatCurrency } from '@/lib/format'

interface EquityChartProps {
  equity: EquityPoint[]
}

export function EquityChart({ equity }: EquityChartProps) {
  const chartData = useMemo(() => {
    return equity.map((point, index) => ({
      ...point,
      label: index + 1,
    }))
  }, [equity])

  return (
    <Card className="glass gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-5 pt-5 pb-4">
        <CardTitle className="text-sm font-semibold text-[var(--text-secondary)]">Equity Curve</CardTitle>
      </CardHeader>

      <CardContent className="h-[240px] px-4 pb-4 sm:px-5 sm:pb-5">
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
                  background: 'rgba(17,24,39,0.95)',
                  border: '1px solid var(--border)',
                  borderRadius: '8px',
                  color: 'var(--text-primary)',
                  fontFamily: '"Geist Pixel", monospace',
                  fontSize: '11px',
                }}
                labelStyle={{ color: 'var(--text-secondary)' }}
              />
              <Area
                type="monotone"
                dataKey="cumulativePnl"
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
