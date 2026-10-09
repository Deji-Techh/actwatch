// actwatch-core: SQLite schema, categories, aggregation queries.
// Retention policy: forever — no delete/prune code lives in this crate.

pub mod categories;
pub mod store;

pub use categories::{Categories, Category};
pub use store::{
    AppStats, DayStack, RollupRow, Store, WeekDelta, SCHEMA_SQL, SCHEMA_VERSION,
};
