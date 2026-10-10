// actwatch-core: SQLite schema, categories, aggregation queries.
// Retention policy: forever — no delete/prune code lives in this crate.
//
// ## Shared config file: `~/.config/actwatch/categories.toml`
//
// `DisplayNames` and `Categories` both read this same TOML file, which
// carries three top-level tables (unknown tables are ignored):
//
// ```toml
// [aliases]
// # Hyprland app-id (case-insensitive) -> friendly display name.
// # Highest priority in DisplayNames::resolve.
// "com.t3tools.T3Code" = "T3Code"
//
// [categories]
// # Hyprland app-id (case-insensitive) -> Category display label.
// # Overrides the builtins in Categories::categorize.
// # Labels: "Social" | "Productivity & Finance" | "Utilities"
// #         | "Gaming" | "Media" | "Other".
// steam = "Gaming"
// vlc = "Media"
//
// # NOTE: the pre-v2 `[apps]` table is still accepted as a legacy alias
// # for `[categories]` ([categories] wins on conflict).
//
// [privacy]
// # titles = "off" | "hash" | "full" (default "off").
// # See TitlePolicy.
// titles = "off"
// ```

pub mod categories;
pub mod model;
pub mod names;
pub mod store;
pub mod titles;

pub use categories::{Categories, Category};
pub use model::{AppEvent, ResourceSample};
pub use names::DisplayNames;
pub use store::{
    AppStats, DayStack, RollupRow, Store, WeekDelta, SCHEMA_SQL, SCHEMA_VERSION,
};
pub use titles::TitlePolicy;
