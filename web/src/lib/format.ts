import type { LedgerFilter, LedgerRow } from '../types'
import { femaAlert, type AlertLevel } from './fema'

/** Integer paise -> "₹41,500.00" (Indian digit grouping). */
export function formatInr(paise: number): string {
  return `₹${(paise / 100).toLocaleString('en-IN', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`
}

export function rowAlert(row: LedgerRow): AlertLevel {
  return femaAlert(row.invoice_date, row.inr_vostro, row.status)
}

export function confidenceTone(c: number | null): 'green' | 'amber' | 'red' | 'none' {
  if (c == null) return 'none'
  if (c >= 85) return 'green'
  if (c >= 60) return 'amber'
  return 'red'
}

/** Apply a LedgerFilter (from NL query) over already-loaded rows, client-side. */
export function applyFilter(rows: LedgerRow[], filter: LedgerFilter | null): LedgerRow[] {
  if (!filter) return rows
  const text = filter.text?.trim().toLowerCase() ?? ''
  return rows.filter((row) => {
    if (filter.status && row.status !== filter.status) return false
    if (filter.purpose_code && row.purpose_code !== filter.purpose_code) return false
    if (filter.min_confidence != null && (row.ai_confidence ?? -1) < filter.min_confidence)
      return false
    if (filter.alert && rowAlert(row) !== filter.alert) return false
    if (text) {
      const haystack = [
        row.client_name,
        row.invoice_number ?? '',
        row.id,
        row.order_id,
        row.ebrc_number ?? '',
        row.purpose_code ?? '',
      ]
        .join(' ')
        .toLowerCase()
      if (!haystack.includes(text)) return false
    }
    return true
  })
}

/** Human-readable description of an active filter, for chips. */
export function filterChips(filter: LedgerFilter | null): string[] {
  if (!filter) return []
  const chips: string[] = []
  if (filter.status) chips.push(`Status: ${filter.status}`)
  if (filter.purpose_code) chips.push(`Purpose: ${filter.purpose_code}`)
  if (filter.min_confidence != null) chips.push(`Confidence ≥ ${filter.min_confidence}`)
  if (filter.alert) chips.push(`FEMA alert: ${filter.alert}`)
  if (filter.text) chips.push(`"${filter.text}"`)
  return chips
}
