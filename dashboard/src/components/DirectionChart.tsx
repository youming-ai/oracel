import { Bar, BarChart, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { DirectionStats } from '@/lib/dashboard-types'
import { ArrowUpDown } from 'lucide-react'

interface DirectionChartProps {
  data: DirectionStats[]
}

export function DirectionChart({ data }: DirectionChartProps) {
  return (
    <Card className="hud-card gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-4 pt-4 pb-3 sm:px-5 sm:pt-5 sm:pb-4">
        <CardTitle className="card-title-hud">
          <ArrowUpDown className="size-3.5 text-[var(--accent)]" />
          Direction Breakdown
        </CardTitle>
      </CardHeader>

      <CardContent className="h-[200px] px-2 pb-3 sm:h-[240px] sm:px-4 sm:pb-5">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={data} barGap={6} margin={{ top: 6, right: 4, left: -14, bottom: 0 }}>
            <CartesianGrid stroke="rgba(30,45,61,0.3)" vertical={false} />
            <XAxis
              dataKey="direction"
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-secondary)', fontFamily: '"Geist Pixel", monospace', fontSize: 11 }}
            />
            <YAxis
              allowDecimals={false}
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-secondary)', fontFamily: '"Geist Pixel", monospace', fontSize: 10 }}
            />
            <Tooltip
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
            <Legend
              iconType="rect"
              wrapperStyle={{
                color: 'var(--text-secondary)',
                fontFamily: '"Geist Pixel", monospace',
                fontSize: '10px',
              }}
            />
            <Bar dataKey="wins" name="Wins" fill="rgba(0,212,170,0.7)" radius={[4, 4, 0, 0]} />
            <Bar dataKey="losses" name="Losses" fill="rgba(255,71,87,0.7)" radius={[4, 4, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}
