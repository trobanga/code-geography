//! What every front end reports about the call sites it read.

use serde::{Deserialize, Serialize};

/// How the call sites of the sources were resolved. Counts of call sites, before the
/// removal of duplicate edges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallReport {
    /// The call reaches one function of the repository.
    pub to_function: usize,
    /// The call reaches the type of the receiver: several overloads fit, or none is declared.
    pub to_type: usize,
    /// The type of the receiver is known and not in the repository.
    pub external: usize,
    /// The type of the receiver is not known.
    pub unresolved: usize,
    /// The reference reaches a value node (a Clojure `def`): an edge `value_ref`.
    #[serde(default)]
    pub to_value: usize,
}

impl std::ops::Add for CallReport {
    type Output = CallReport;

    fn add(self, other: CallReport) -> CallReport {
        CallReport {
            to_function: self.to_function + other.to_function,
            to_type: self.to_type + other.to_type,
            external: self.external + other.external,
            unresolved: self.unresolved + other.unresolved,
            to_value: self.to_value + other.to_value,
        }
    }
}
