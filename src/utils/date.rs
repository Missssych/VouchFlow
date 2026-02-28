//! Date normalization helpers for stock expiry.

use chrono::NaiveDate;

const INPUT_DATE_FORMATS: [&str; 4] = ["%Y-%m-%d", "%d-%m-%Y", "%Y/%m/%d", "%d/%m/%Y"];
const NORMALIZED_DATE_FORMAT: &str = "%Y-%m-%d";

fn first_date_token(raw: &str) -> &str {
    raw.split(|c| c == 'T' || c == ' ')
        .next()
        .unwrap_or(raw)
        .trim()
}

/// Normalize optional expiry input into canonical `YYYY-MM-DD`.
///
/// Accepted input formats:
/// - `YYYY-MM-DD`
/// - `DD-MM-YYYY`
/// - `YYYY/MM/DD`
/// - `DD/MM/YYYY`
///
/// Also supports date-time strings by taking only the first date token
/// before `T` or space (e.g. `2026-03-01T00:00:00Z` -> `2026-03-01`).
pub fn normalize_expired_date_optional(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let candidate = first_date_token(trimmed);
    for fmt in INPUT_DATE_FORMATS {
        if let Ok(parsed) = NaiveDate::parse_from_str(candidate, fmt) {
            return Ok(Some(parsed.format(NORMALIZED_DATE_FORMAT).to_string()));
        }
    }

    Err(
        "Format expired_date tidak valid. Gunakan YYYY-MM-DD, DD-MM-YYYY, YYYY/MM/DD, atau DD/MM/YYYY."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::normalize_expired_date_optional;

    #[test]
    fn normalize_accepts_iso() {
        let out = normalize_expired_date_optional("2026-03-01").unwrap();
        assert_eq!(out.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn normalize_accepts_day_first_dash() {
        let out = normalize_expired_date_optional("01-03-2026").unwrap();
        assert_eq!(out.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn normalize_accepts_iso_slash() {
        let out = normalize_expired_date_optional("2026/03/01").unwrap();
        assert_eq!(out.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn normalize_accepts_day_first_slash() {
        let out = normalize_expired_date_optional("01/03/2026").unwrap();
        assert_eq!(out.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn normalize_accepts_provider_datetime() {
        let out = normalize_expired_date_optional("2026-03-01T12:34:56+07:00").unwrap();
        assert_eq!(out.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn normalize_accepts_empty() {
        let out = normalize_expired_date_optional("   ").unwrap();
        assert_eq!(out, None);
    }

    #[test]
    fn normalize_rejects_invalid() {
        let out = normalize_expired_date_optional("03.01.2026");
        assert!(out.is_err());
    }
}
