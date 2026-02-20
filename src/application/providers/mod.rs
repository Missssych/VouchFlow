//! Provider implementations - Each provider has check/redeem functions
//! 
//! Architecture: Provider-based instead of function-based
//! - Each provider (Byu, Smartfren, Telkomsel) has its own module
//! - All providers implement the ProviderApi trait
//! - ProviderRouter routes to the correct provider based on product

pub mod traits;
pub mod byu;
pub mod smartfren;
pub mod telkomsel;
pub mod router;

pub use traits::*;
pub use byu::ByuProvider;
pub use smartfren::SmartfrenProvider;
pub use telkomsel::TelkomselProvider;
pub use router::ProviderRouter;

