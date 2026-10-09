//! Open Model Catalog: the data model, loader, validator and JSON exports.
//!
//! This crate performs no I/O other than reading the data directory
//! ([`load`]). Exports are pure functions from a validated [`Catalog`] to
//! bytes, so `omg-models build` and `omg-models serve` produce identical
//! output.

pub mod decimal;
pub mod export;
pub mod load;
pub mod meter;
pub mod model;
pub mod schema;
pub mod time;
pub mod validate;

use std::{fmt, path::Path};

pub use decimal::Decimal;
pub use load::{Catalog, Model, Provider};
pub use meter::Meter;

/// How serious an [`Issue`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// A validation finding tied to a data file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// File path relative to the data directory.
    pub path: String,
    /// Location inside the file, e.g. `offerings[0].prices[1]`.
    pub at: Option<String>,
    pub message: String,
}

impl Issue {
    pub fn error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            path: path.into(),
            at: None,
            message: message.into(),
        }
    }

    pub fn warning(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(path, message)
        }
    }

    pub fn error_at(
        path: impl Into<String>,
        at: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            at: Some(at.into()),
            ..Self::error(path, message)
        }
    }

    pub fn warning_at(
        path: impl Into<String>,
        at: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            at: Some(at.into()),
            ..Self::warning(path, message)
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        match &self.at {
            Some(at) => write!(f, "{level}: {}: {at}: {}", self.path, self.message),
            None => write!(f, "{level}: {}: {}", self.path, self.message),
        }
    }
}

/// Loads and validates `data_dir`. Returns the catalog when there are no
/// errors, plus every issue (warnings included).
pub fn load_validated(data_dir: &Path) -> (Option<Catalog>, Vec<Issue>) {
    let (catalog, mut issues) = load::load(data_dir);
    issues.extend(validate::validate(&catalog));
    issues.sort_by(|a, b| (&a.path, &a.at, &a.message).cmp(&(&b.path, &b.at, &b.message)));
    let ok = !issues.iter().any(|i| i.severity == Severity::Error);
    (ok.then_some(catalog), issues)
}
