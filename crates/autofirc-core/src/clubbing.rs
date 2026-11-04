use chrono::NaiveDate;
use thiserror::Error;

/// An Inward Remittance Message uploaded by a bank to DGFT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Irm {
    pub irm_number: String,
    pub bank_name: String,
    pub account_number: String,
    pub currency: String,
    pub remittance_date: NaiveDate,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClubError {
    #[error("IRMs from different banks cannot be clubbed")]
    DifferentBank,
    #[error("IRMs into different accounts cannot be clubbed")]
    DifferentAccount,
    #[error("IRMs in different currencies cannot be clubbed")]
    DifferentCurrency,
    #[error("advance payment: invoice date {invoice} precedes remittance date {remittance}")]
    AdvanceBeforeInvoice {
        invoice: NaiveDate,
        remittance: NaiveDate,
    },
}

/// Two IRMs may be clubbed into a single eBRC only under DGFT's rules.
pub fn can_club(a: &Irm, b: &Irm) -> Result<(), ClubError> {
    if a.bank_name != b.bank_name {
        return Err(ClubError::DifferentBank);
    }
    if a.account_number != b.account_number {
        return Err(ClubError::DifferentAccount);
    }
    if a.currency != b.currency {
        return Err(ClubError::DifferentCurrency);
    }
    Ok(())
}

/// For advance payments the invoice date must be on/after the remittance date.
pub fn validate_advance(
    is_advance: bool,
    invoice_date: NaiveDate,
    remittance_date: NaiveDate,
) -> Result<(), ClubError> {
    if is_advance && invoice_date < remittance_date {
        return Err(ClubError::AdvanceBeforeInvoice {
            invoice: invoice_date,
            remittance: remittance_date,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn irm(bank: &str, acct: &str, ccy: &str) -> Irm {
        Irm {
            irm_number: "IRM-1".into(),
            bank_name: bank.into(),
            account_number: acct.into(),
            currency: ccy.into(),
            remittance_date: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
        }
    }

    #[test]
    fn clubbing_requires_same_bank_account_currency() {
        assert!(can_club(&irm("HDFC", "001", "USD"), &irm("HDFC", "001", "USD")).is_ok());
        assert_eq!(
            can_club(&irm("HDFC", "001", "USD"), &irm("ICICI", "001", "USD")),
            Err(ClubError::DifferentBank)
        );
        assert_eq!(
            can_club(&irm("HDFC", "001", "USD"), &irm("HDFC", "002", "USD")),
            Err(ClubError::DifferentAccount)
        );
        assert_eq!(
            can_club(&irm("HDFC", "001", "USD"), &irm("HDFC", "001", "EUR")),
            Err(ClubError::DifferentCurrency)
        );
    }

    #[test]
    fn advance_payment_needs_invoice_on_or_after_remittance() {
        let inv = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let rem = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        assert!(validate_advance(true, inv, rem).is_ok());
        assert!(validate_advance(false, rem, inv).is_ok()); // non-advance: no constraint
        assert!(matches!(
            validate_advance(true, rem, inv),
            Err(ClubError::AdvanceBeforeInvoice { .. })
        ));
    }
}
