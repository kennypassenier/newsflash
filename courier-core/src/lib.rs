//! Pure decision logic for the hub-clients couriers (AR3): envelope
//! parsing, the settle table, dedup bookkeeping, toast mapping and the
//! backoff schedule. No ambient I/O — everything here is testable
//! without a hub, a desktop or a filesystem.

pub mod action_result;
pub mod backoff;
pub mod dedup;
pub mod envelope;
pub mod hub;
pub mod settle;
pub mod toast;
