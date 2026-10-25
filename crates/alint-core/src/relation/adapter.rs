//! The adapter trait: a resource's bytes in, rows out.

use super::limits::{LimitExceeded, LimitKind, Limits};
use super::resource::Resource;
use super::row::Row;

/// Why an adapter produced no rows for a resource. Each is an analysis failure
/// for that resource (DEC-2), never an empty result that passes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AdapterError {
    #[error("{0}")]
    Parse(String),
    #[error(transparent)]
    Limit(#[from] LimitExceeded),
}

/// Turns one resource's bytes into rows. Implementations are stateless and
/// shared across threads; R2 caches their output so each (adapter, resource)
/// pair is decoded at most once per run.
pub trait Adapter: Send + Sync + std::fmt::Debug {
    /// A stable identity, used for the decode cache and in provenance.
    fn id(&self) -> &'static str;

    /// Every row of `resource`. Implementations stop and return
    /// [`AdapterError::Limit`] once they would exceed
    /// [`LimitKind::RowsPerExtractor`] (see [`check_row_count`]).
    fn rows(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<Vec<Row>, AdapterError>;
}

/// The per-extractor row limit, for adapters to call as they grow their output.
pub fn check_row_count(limits: &Limits, rows: usize) -> Result<(), AdapterError> {
    Ok(limits.check(LimitKind::RowsPerExtractor, rows)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::{Pointer, RowState, Value};

    /// One row per line, the smallest adapter that exercises the contract.
    #[derive(Debug)]
    struct Lines;

    impl Adapter for Lines {
        fn id(&self) -> &'static str {
            "test-lines"
        }

        fn rows(
            &self,
            resource: &Resource,
            bytes: &[u8],
            limits: &Limits,
        ) -> Result<Vec<Row>, AdapterError> {
            let text =
                std::str::from_utf8(bytes).map_err(|e| AdapterError::Parse(e.to_string()))?;
            let mut rows = Vec::new();
            for (index, line) in text.lines().enumerate() {
                check_row_count(limits, rows.len() + 1)?;
                rows.push(Row::new(
                    resource.clone(),
                    Some(Pointer::root().index(index)),
                    u32::try_from(index).expect("a test file has few lines"),
                    RowState::Value(Value::String(line.into())),
                ));
            }
            Ok(rows)
        }
    }

    #[test]
    fn an_adapter_reports_parse_failures_and_limits_instead_of_empty_rows() {
        let resource = Resource::for_test(0, "a.txt");
        let limits = Limits::default();
        assert_eq!(Lines.rows(&resource, b"a\nb", &limits).unwrap().len(), 2);
        assert!(matches!(
            Lines.rows(&resource, &[0xff], &limits),
            Err(AdapterError::Parse(_))
        ));
        let tight = Limits::default()
            .with(LimitKind::RowsPerExtractor, 2)
            .unwrap();
        assert_eq!(
            Lines.rows(&resource, b"a\nb\nc", &tight),
            Err(AdapterError::Limit(LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit: 2,
                reached: 3
            }))
        );
    }
}
