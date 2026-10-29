//! Third-party services behind FUber-owned interfaces (TRD §8). Each has a fake or local
//! implementation, so the whole system runs end to end in development and CI without vendor
//! accounts; real vendors are added as variants once they are chosen (PRD §10).

pub mod media;
pub mod routing;
pub mod sms;
pub mod storage;
pub mod virus;
