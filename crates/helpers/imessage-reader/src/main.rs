//! `imessage-reader`: reads Apple Messages for the Message Vault desktop app.
//!
//! This program exists for a licence reason, not a product one. It links
//! `imessage-database` and `crabapple`, which are GPL-3.0-or-later, and the
//! desktop app is under the Fair Core License, so the two cannot be one
//! binary. The app starts this one, writes a request on its stdin, and reads
//! events off its stdout; the protocol is `imessage-reader-protocol`. Nothing
//! here is meant to be typed at a shell, and ADR 0001's rule (no command line
//! except the vault server) still stands: an internal helper the app spawns
//! is not a command line for people.
//!
//! What it does: opens `chat.db` (or decrypts an iPhone backup), caches
//! chats, handles, contacts and tapbacks, then streams every message as an
//! already-classified record. Turning those records into the shared
//! conversation structure, writing files, media handling and everything else
//! the product does stays in the app.

mod attachments;
mod attachments_emit;
mod backup;
mod body;
mod contacts;
mod data_source;
mod emit;
mod error;
mod fields;
mod identities;
mod log;
mod options;
mod session;
#[cfg(test)]
mod test_support;

use std::io::BufRead;

use imessage_reader_protocol::{Event, PROTOCOL_VERSION, Request};

use crate::{log::emit, options::ReaderOptions, session::MailSession};

/// Report a failure to the app and stop.
fn fail(message: impl ToString) -> ! {
    emit(&Event::Error {
        message: message.to_string(),
    });
    std::process::exit(1)
}

fn main() {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    // No request means the app changed its mind; there is nothing to report.
    let Some(first) = lines.next() else {
        return;
    };
    let first = first.unwrap_or_else(|e| fail(format!("could not read the request: {e}")));
    let request: Request = serde_json::from_str(&first)
        .unwrap_or_else(|e| fail(format!("the request is not valid JSON: {e}")));

    match request {
        Request::Identities(source) => match identities::raw_identities(source) {
            Ok(values) => emit(&Event::Identities { values }),
            Err(e) => fail(e),
        },
        Request::Export(request) => {
            let options = ReaderOptions::from_export(request);
            let session = MailSession::new(options).unwrap_or_else(|e| fail(e));
            emit(&Event::Source {
                protocol_version: PROTOCOL_VERSION,
                encrypted: session.data_source.is_encrypted(),
            });
            emit::stream_export(&session).unwrap_or_else(|e| fail(e));
            // The export is streamed. An encrypted backup's attachments still
            // need this process to decrypt them, so stay for those requests
            // until the app closes stdin.
            for line in lines {
                let line = line.unwrap_or_else(|e| fail(format!("could not read a request: {e}")));
                match serde_json::from_str::<Request>(&line) {
                    Ok(Request::Attachment { path }) => {
                        emit(&attachments::decrypt_for_app(&session, &path));
                    }
                    Ok(_) => fail("only attachment requests may follow an export"),
                    Err(e) => fail(format!("the request is not valid JSON: {e}")),
                }
            }
        }
        Request::Attachment { .. } => fail("an attachment request needs an export first"),
    }
}
