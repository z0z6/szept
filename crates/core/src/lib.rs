//! Core of the messenger: identity, contacts, wire protocol and networking.
//!
//! Everything a user interface needs lives here, so the terminal client and
//! the future graphical client share one implementation.

pub mod contacts;
pub mod identity;
pub mod node;
pub mod protocol;

pub use contacts::Contacts;
pub use identity::Identity;
pub use iroh::{EndpointAddr, EndpointId};
pub use node::{Event, Network, Node};
pub use protocol::{MessageId, Outgoing};
