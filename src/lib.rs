#![allow(clippy::needless_return, clippy::let_and_return)]

mod aliases;
mod desugar;
mod lex;
mod mask;
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
pub use desugar::source_span;
pub use mask::Masked;
pub use mask::mask;
pub use mask::unmask;
pub use strip::StripError;
pub use strip::strip;
pub use types::title_case_types;
pub use units::ByteSpan;
