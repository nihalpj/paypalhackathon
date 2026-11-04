import { useCallback, useMemo } from 'react'
import {
  AllCommunityModule,
  colorSchemeDark,
  ModuleRegistry,
  themeQuartz,
  type CellClassParams,
  type ColDef,
  type ICellRendererParams,
  type RowSelectionOptions,
} from 'ag-grid-community'
import { AgGridReact } from 'ag-grid-react'
import type { LedgerRow } from '../types'
import { STATUS_LABELS } from '../types'
import { formatInr, rowAlert } from '../lib/format'
import { femaDeadline } from '../lib/fema'
import { format } from 'date-fns'

ModuleRegistry.registerModules([AllCommunityModule])

const gridTheme = themeQuartz
  .withPart(colorSchemeDark)
  .withParams({
    backgroundColor: 'transparent',
    foregroundColor: '#C7D2E8',
    borderColor: '#1D2A47',
    headerBackgroundColor: '#0E1730',
    headerTextColor: '#8FA3C8',
    oddRowBackgroundColor: 'rgba(255,255,255,0.015)',
    accentColor: '#3B7BFF',
    rowHoverColor: 'rgba(59,123,255,0.08)',
    fontFamily: 'inherit',
    headerFontSize: 12,
  })

function AlertCell(p: ICellRendererParams<LedgerRow>) {
  const row = p.data
  if (!row) return null
  const level = rowAlert(row)
  const days = Math.round(
    (femaDeadline(row.invoice_date, row.inr_vostro).getTime() - Date.now()) / 86_400_000,
  )
  const label =
    row.status === 'fully_reconciled'
      ? 'Reconciled'
      : days < 0
        ? `${-days}d overdue`
        : `${days}d left`
  return (
    <span className={`alert-badge alert-${level}`}>
      <span className="alert-dot" aria-hidden />
      {label}
    </span>
  )
}

function ConfidenceCell(p: ICellRendererParams<LedgerRow>) {
  const c = p.data?.ai_confidence
  if (c == null) return <span className="muted">—</span>
  const tone = c >= 85 ? 'green' : c >= 60 ? 'amber' : 'red'
  return (
    <span className={`conf conf-${tone}`} title={`AI confidence ${c}%`}>
      <span className="conf-bar" aria-hidden>
        <span className="conf-fill" style={{ width: `${c}%` }} />
      </span>
      {c}%
    </span>
  )
}

function StatusCell(p: ICellRendererParams<LedgerRow>) {
  const status = p.data?.status
  if (!status) return null
  return <span className={`status-chip status-${status}`}>{STATUS_LABELS[status]}</span>
}

function MonospaceCell(p: ICellRendererParams<LedgerRow>, key: keyof LedgerRow) {
  const v = p.data?.[key]
  return v ? <span className="mono">{String(v)}</span> : <span className="muted">—</span>
}

export interface LedgerGridProps {
  rows: LedgerRow[]
  onRowClicked: (row: LedgerRow) => void
  onSelectionChanged: (ids: string[]) => void
}

export default function LedgerGrid({ rows, onRowClicked, onSelectionChanged }: LedgerGridProps) {
  const columnDefs = useMemo<ColDef<LedgerRow>[]>(
    () => [
      {
        field: 'client_name',
        headerName: 'Client',
        flex: 1.4,
        minWidth: 160,
        pinned: 'left',
      },
      {
        field: 'amount_inr_paise',
        headerName: 'Amount (INR)',
        valueFormatter: (p) => formatInr(p.value as number),
        type: 'rightAligned',
        flex: 1,
        minWidth: 120,
        cellClass: 'amount-cell',
      },
      {
        field: 'invoice_date',
        headerName: 'Invoice date',
        cellRenderer: (p: ICellRendererParams<LedgerRow>) =>
          typeof p.value === 'string' ? <span className="mono">{p.value}</span> : null,
        width: 120,
      },
      {
        field: 'purpose_code',
        headerName: 'Purpose code',
        cellRenderer: (p: ICellRendererParams<LedgerRow>) => MonospaceCell(p, 'purpose_code'),
        width: 120,
      },
      {
        field: 'sac_code',
        headerName: 'SAC',
        cellRenderer: (p: ICellRendererParams<LedgerRow>) => MonospaceCell(p, 'sac_code'),
        width: 100,
      },
      {
        field: 'ai_confidence',
        headerName: 'AI conf.',
        cellRenderer: ConfidenceCell,
        width: 120,
      },
      {
        headerName: 'FEMA deadline',
        valueGetter: (p) =>
          p.data ? format(femaDeadline(p.data.invoice_date, p.data.inr_vostro), 'dd MMM yyyy') : '',
        cellRenderer: (p: ICellRendererParams) => (
          <span className="mono">{p.value as string}</span>
        ),
        width: 130,
      },
      {
        headerName: 'FEMA alert',
        cellRenderer: AlertCell,
        cellClass: (p: CellClassParams) => `alert-cell-${rowAlert(p.data as LedgerRow)}`,
        width: 120,
      },
      {
        field: 'status',
        headerName: 'Status',
        cellRenderer: StatusCell,
        width: 150,
      },
      {
        field: 'ebrc_number',
        headerName: 'eBRC',
        cellRenderer: (p: ICellRendererParams<LedgerRow>) => MonospaceCell(p, 'ebrc_number'),
        width: 110,
      },
    ],
    [],
  )

  const rowSelection = useMemo<RowSelectionOptions<LedgerRow>>(
    () => ({
      mode: 'multiRow',
      checkboxes: true,
      headerCheckbox: true,
      isRowSelectable: (p) => p.data?.status === 'compliance_packet_ready',
    }),
    [],
  )

  const handleRowClicked = useCallback(
    (e: { data?: LedgerRow }) => {
      if (e.data) onRowClicked(e.data)
    },
    [onRowClicked],
  )

  return (
    <div className="grid-wrap">
      <AgGridReact<LedgerRow>
        theme={gridTheme}
        columnDefs={columnDefs}
        rowData={rows}
        getRowId={(p) => p.data.id}
        rowSelection={rowSelection}
        onRowClicked={handleRowClicked}
        onSelectionChanged={(e) => onSelectionChanged(e.api.getSelectedRows().map((r) => r.id))}
        suppressCellFocus
        defaultColDef={{ resizable: true, sortable: true }}
      />
    </div>
  )
}
