//! Decode GO SMS Pro MMS protocol data units (PDUs).
//!
//! GO SMS Pro is an Android messaging app. Its backup folder holds the SMS
//! in one XML file and each MMS as a `.pdu` file: the MMS PDU the phone's
//! MMS stack held, byte for byte. This crate decodes those files by the
//! WAP-209 and WAP-230 rules and nothing else. The layers, bottom up:
//!
//! - [`wsp`]: the WSP value shapes and the multipart body (WAP-230).
//! - [`mms`]: the MMS header walk and the body (WAP-209).
//! - [`pdu`]: one file as one message: direction, people, text, attachments.
//!
//! Each module's documentation lists the rules it holds to and why. The
//! [`testutil`] module (feature `testutil`) builds PDUs the way a phone
//! writes them, for tests here and in the exporter.

mod emoji;
pub mod mms;
pub mod pdu;
pub mod wsp;

#[cfg(any(test, feature = "testutil"))]
pub mod testutil;

pub use emoji::decode_gosms_emojis;
pub use pdu::{ParsedAttachment, ParsedPdu, PduError, parse_pdu_bytes, parse_pdu_file};

#[cfg(test)]
mod robustness_tests;
