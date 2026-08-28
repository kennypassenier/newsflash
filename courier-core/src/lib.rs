//! Pure decision logic for the hub-clients couriers (AR3): envelope
//! parsing, the settle table, dedup bookkeeping, toast mapping and the
//! backoff schedule. No ambient I/O — everything here is testable
//! without a hub, a desktop or a filesystem.
