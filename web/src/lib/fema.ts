import { addMonths, differenceInCalendarDays, parseISO } from 'date-fns';

export type AlertLevel = 'green' | 'amber' | 'red';

/** FEMA realization deadline: invoice_date + 9 months (12 for INR vostro accounts). */
export function femaDeadline(invoiceDate: string, inrVostro: boolean): Date {
  return addMonths(parseISO(invoiceDate), inrVostro ? 12 : 9);
}

/**
 * FEMA alert level for a ledger row.
 * - Reconciled rows are always green.
 * - RED when <= 31 days remain, AMBER when <= 62 days remain, else GREEN.
 * Days remaining uses calendar days from today to the deadline (inclusive of the deadline day).
 */
export function femaAlert(
  invoiceDate: string,
  inrVostro: boolean,
  status: string,
  today: Date = new Date(),
): AlertLevel {
  if (status === 'fully_reconciled') return 'green';
  const daysRemaining = differenceInCalendarDays(femaDeadline(invoiceDate, inrVostro), today);
  if (daysRemaining <= 31) return 'red';
  if (daysRemaining <= 62) return 'amber';
  return 'green';
}
