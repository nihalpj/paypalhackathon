import type { DocumentsResponse, LedgerFilter, LedgerRow, PetitionResponse } from '../types'

// Same-origin in dev (Vite proxy); absolute URL from the build env in prod.
const API = (import.meta.env.VITE_API_URL ?? '').replace(/\/$/, '')

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    let detail = ''
    try {
      detail = await res.text()
    } catch {
      // ignore body read failure
    }
    throw new Error(`${res.status} ${res.statusText}${detail ? `: ${detail.slice(0, 200)}` : ''}`)
  }
  return res.json() as Promise<T>
}

export async function fetchLedger(): Promise<LedgerRow[]> {
  const res = await fetch(`${API}/api/ledger`)
  return json<LedgerRow[]>(res)
}

export async function fetchDocuments(id: string): Promise<DocumentsResponse> {
  const res = await fetch(`${API}/api/ledger/${encodeURIComponent(id)}/documents`)
  return json<DocumentsResponse>(res)
}

export async function postPetition(eventIds: string[]): Promise<PetitionResponse> {
  const res = await fetch(`${API}/api/petition`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ event_ids: eventIds }),
  })
  return json<PetitionResponse>(res)
}

/**
 * Hosted demo checkout page (create + capture in one PayPal SDK flow).
 * Empty API base falls back to the local default server port.
 */
export function checkoutUrl(params: {
  amount: string
  currency: string
  description: string
}): string {
  const base = API || 'http://localhost:8080'
  const q = new URLSearchParams(params)
  return `${base}/checkout?${q.toString()}`
}

/**
 * Natural-language ledger filter. Throws when no LLM key is configured (503);
 * callers fall back to client-side text search.
 */
export async function postQuery(query: string): Promise<LedgerFilter> {
  const res = await fetch(`${API}/api/query`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query }),
  })
  return json<LedgerFilter>(res)
}
