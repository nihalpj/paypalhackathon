export interface LedgerRow {
  id: string
  capture_id: string
  order_id: string
  client_name: string
  amount_inr_paise: number
  invoice_date: string // "YYYY-MM-DD"
  inr_vostro: boolean
  purpose_code: string | null
  mode_of_export: number | null // GATS mode 1-4
  sac_code: string | null
  ai_confidence: number | null // 0-100
  ebrc_number: string | null
  status: LedgerStatus
  invoice_number: string | null
  backstop_note: string | null
  enrichment: Record<string, unknown> | null
  created_at: string
  updated_at: string
}

export type LedgerStatus =
  | 'pending_analysis'
  | 'enriched'
  | 'manual_review'
  | 'compliance_packet_ready'
  | 'petition_authorized'
  | 'bank_petitioned'
  | 'fully_reconciled'
  | 'domestic_rejected'

export const PIPELINE: LedgerStatus[] = [
  'pending_analysis',
  'enriched',
  'manual_review',
  'compliance_packet_ready',
  'petition_authorized',
  'bank_petitioned',
  'fully_reconciled',
]

export interface LedgerFilter {
  status?: string | null
  purpose_code?: string | null
  min_confidence?: number | null
  alert?: 'green' | 'amber' | 'red' | null
  text?: string | null
}

export interface DocumentsResponse {
  invoice_url: string | null
  lut_url: string | null
}

export interface PetitionResponse {
  queued: number
  skipped: string[]
}

export const STATUS_LABELS: Record<LedgerStatus, string> = {
  pending_analysis: 'Pending analysis',
  enriched: 'Enriched',
  manual_review: 'Manual review',
  compliance_packet_ready: 'Packet ready',
  petition_authorized: 'Authorized',
  bank_petitioned: 'Bank petitioned',
  fully_reconciled: 'Reconciled',
  domestic_rejected: 'Domestic ✕',
}
