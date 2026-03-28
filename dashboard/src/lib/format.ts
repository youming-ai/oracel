export function formatCurrency(value: number, decimals = 2): string {
  return `$${value.toLocaleString(undefined, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  })}`
}

export function formatPercent(value: number, decimals = 1): string {
  return `${value.toFixed(decimals)}%`
}

export function formatBtc(value: number | null): string {
  if (value === null) {
    return '—'
  }

  return formatCurrency(value, 0)
}

export function formatTime(date: Date | null): string {
  if (!date) {
    return '—'
  }

  return date.toLocaleTimeString('ja-JP')
}
