#![allow(clippy::needless_return, clippy::let_and_return)]

mod aliases;
mod desugar;
mod lex;
mod names;
mod sites;
mod strip;
mod types;
mod units;

pub use aliases::AliasTable;
pub use aliases::TypeAlias;
pub use aliases::alias_table;
pub use aliases::type_aliases;
pub use desugar::Desugared;
pub use desugar::desugar;
pub use strip::StripError;
pub use strip::strip;
pub use units::ByteSpan;
