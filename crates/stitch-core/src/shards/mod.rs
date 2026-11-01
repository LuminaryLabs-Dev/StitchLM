//! Data-driven shard execution contracts. Definitions, executions and measurements
//! have separate lifetimes; metadata never substitutes for a supported kernel.
pub mod assembly;
pub mod definition;
pub mod execution;
pub mod scheduler;
pub use assembly::*;
pub use definition::*;
pub use execution::*;
pub use scheduler::*;
pub mod split;
pub use split::*;
#[cfg(test)]
mod tests;
