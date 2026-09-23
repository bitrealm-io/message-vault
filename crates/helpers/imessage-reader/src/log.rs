//! Events go to stdout, one JSON line each. Log lines are events too, so the
//! app sees them in the order they happened relative to the messages.

use std::io::Write;

use imessage_reader_protocol::Event;

/// Write one event line to stdout and flush it, so the app reads it now
/// rather than when the buffer fills.
pub(crate) fn emit(event: &Event) {
    #[cfg(test)]
    if capture::record(event) {
        return;
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    // A failed write means the app has gone away; there is nobody left to
    // tell, and the next read of stdin ends the process.
    let _ = serde_json::to_writer(&mut out, event);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

/// Send one log line to the app.
pub(crate) fn emit_log(line: impl AsRef<str>) {
    emit(&Event::Log {
        line: line.as_ref().to_string(),
    });
}

/// Tests read what the helper would have written: inside
/// [`capture::events`], each event on this thread is kept as JSON instead of
/// going to stdout.
#[cfg(test)]
pub(crate) mod capture {
    use std::cell::RefCell;

    use imessage_reader_protocol::Event;
    use serde_json::Value;

    thread_local! {
        static EVENTS: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
    }

    /// Keep `event` if a capture is running on this thread.
    pub(super) fn record(event: &Event) -> bool {
        EVENTS.with_borrow_mut(|events| match events {
            Some(events) => {
                events.push(serde_json::to_value(event).expect("an event serializes"));
                true
            }
            None => false,
        })
    }

    /// Run `f` and return every event it emitted, in order.
    pub(crate) fn events(f: impl FnOnce()) -> Vec<Value> {
        EVENTS.with_borrow_mut(|events| *events = Some(Vec::new()));
        f();
        EVENTS.with_borrow_mut(Option::take).unwrap_or_default()
    }
}
