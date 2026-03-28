import { useMemo, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import type { TradeEntry, TradeRecord, TradeSettlement } from '@/lib/dashboard-types'
import { formatBtc, formatCurrency } from '@/lib/format'

type TradeFilter = 'all' | 'WIN' | 'LOSS' | 'pending'

interface TradesTableProps {
  trades: TradeRecord[]
  pendingTrades: TradeEntry[]
}

function isSettlement(trade: TradeRecord): trade is TradeSettlement {
  return trade.type === 'settlement'
}

function isEntry(trade: TradeRecord): trade is TradeEntry {
  return trade.type === 'entry'
}

export function TradesTable({ trades, pendingTrades }: TradesTableProps) {
  const [filter, setFilter] = useState<TradeFilter>('all')

  const settlements = useMemo(
    () => trades.filter(isSettlement),
    [trades],
  )

  const filteredTrades = useMemo(() => {
    if (filter === 'WIN') {
      return settlements.filter((trade) => trade.result === 'WIN').reverse()
    }

    if (filter === 'LOSS') {
      return settlements.filter((trade) => trade.result === 'LOSS').reverse()
    }

    if (filter === 'pending') {
      return [...pendingTrades].reverse()
    }

    return [...pendingTrades, ...settlements].reverse().slice(0, 100)
  }, [filter, pendingTrades, settlements])

  return (
    <Card className="glass gap-0 border-0 py-0 ring-0">
      <CardHeader className="flex flex-col justify-between gap-4 border-0 px-5 pt-5 pb-4 md:flex-row md:items-center">
        <CardTitle className="text-sm font-semibold text-[var(--text-secondary)]">Recent Trades</CardTitle>

        <div className="flex flex-wrap gap-2">
          <FilterButton label="All" value="all" activeFilter={filter} onClick={setFilter} />
          <FilterButton label="Wins" value="WIN" activeFilter={filter} onClick={setFilter} />
          <FilterButton label="Losses" value="LOSS" activeFilter={filter} onClick={setFilter} />
          <FilterButton label="Pending" value="pending" activeFilter={filter} onClick={setFilter} />
        </div>
      </CardHeader>

      <CardContent className="px-0 pb-0">
        <div className="scrollbar-thin max-h-[400px] overflow-auto">
          <Table className="mono text-xs">
            <TableHeader className="sticky top-0 z-10 bg-[var(--bg-card)]">
              <TableRow className="border-b border-[rgba(30,45,61,0.5)] hover:bg-transparent">
                <TableHead className="h-9 px-3 text-[var(--text-dim)]">Time</TableHead>
                <TableHead className="h-9 px-3 text-[var(--text-dim)]">Status</TableHead>
                <TableHead className="h-9 px-3 text-[var(--text-dim)]">Direction</TableHead>
                <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">PnL</TableHead>
                <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Entry BTC</TableHead>
                <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Exit BTC</TableHead>
                <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Price</TableHead>
                <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Edge</TableHead>
                <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Payoff</TableHead>
              </TableRow>
            </TableHeader>

            <TableBody>
              {filteredTrades.map((trade) => (
                <TableRow key={buildTradeKey(trade)} className="trade-row border-b-0">
                  <TableCell className="px-3 py-2 text-[var(--text-secondary)]">{trade.time}</TableCell>
                  <TableCell className="px-3 py-2">{renderStatusBadge(trade)}</TableCell>
                  <TableCell className="px-3 py-2">{renderDirectionBadge(trade.direction)}</TableCell>
                  <TableCell className="px-3 py-2 text-right">{renderPnl(trade)}</TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-secondary)]">
                    {isSettlement(trade) ? formatBtc(trade.entryBTC) : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-secondary)]">
                    {isSettlement(trade) ? formatBtc(trade.exitBTC) : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-dim)]">
                    {trade.price !== null ? trade.price.toFixed(3) : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-dim)]">
                    {trade.edge !== null ? `${trade.edge.toFixed(1)}%` : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-dim)]">
                    {trade.payoff ?? '—'}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  )
}

interface FilterButtonProps {
  label: string
  value: TradeFilter
  activeFilter: TradeFilter
  onClick: (value: TradeFilter) => void
}

function FilterButton({ label, value, activeFilter, onClick }: FilterButtonProps) {
  const active = value === activeFilter

  return (
    <Button
      size="xs"
      variant="ghost"
      className="mono rounded-md px-3 py-1 text-xs"
      style={{
        color: active ? 'var(--accent)' : 'var(--text-dim)',
        background: active ? 'var(--accent-dim)' : 'transparent',
      }}
      onClick={() => onClick(value)}
    >
      {label}
    </Button>
  )
}

function renderStatusBadge(trade: TradeRecord) {
  if (isEntry(trade)) {
    return (
      <Badge className="rounded-md border-0 bg-[rgba(255,165,2,0.15)] px-2 py-0.5 text-[11px] text-[var(--warn)]">
        ⏳ PENDING
      </Badge>
    )
  }

  const win = trade.result === 'WIN'
  return (
    <Badge
      className="rounded-md border-0 px-2 py-0.5 text-[11px]"
      style={{
        background: win ? 'rgba(0,212,170,0.15)' : 'rgba(255,71,87,0.15)',
        color: win ? 'var(--win)' : 'var(--loss)',
      }}
    >
      {win ? '✅ WIN' : '❌ LOSS'}
    </Badge>
  )
}

function renderDirectionBadge(direction: 'UP' | 'DOWN') {
  const up = direction === 'UP'
  return (
    <Badge
      className="rounded-md border-0 px-2 py-0.5 text-[11px]"
      style={{
        background: up ? 'rgba(0,212,170,0.15)' : 'rgba(255,71,87,0.15)',
        color: up ? 'var(--win)' : 'var(--loss)',
      }}
    >
      {direction}
    </Badge>
  )
}

function renderPnl(trade: TradeRecord) {
  if (isEntry(trade)) {
    return <span className="text-[var(--text-dim)]">—</span>
  }

  const win = trade.result === 'WIN'
  return (
    <span className="font-semibold" style={{ color: win ? 'var(--win)' : 'var(--loss)' }}>
      {win ? '+' : '-'}{formatCurrency(Math.abs(trade.pnl))}
    </span>
  )
}

function buildTradeKey(trade: TradeRecord): string {
  if (isEntry(trade)) {
    return `entry-${trade.time}-${trade.direction}-${trade.conditionId}`
  }

  return `settlement-${trade.time}-${trade.direction}-${trade.result}-${trade.pnl}`
}
