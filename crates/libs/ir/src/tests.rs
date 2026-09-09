//! Emptiness, which decides whether a message's extensions are written at all.
//!
//! `IrImessage::is_empty` is twenty `&&` clauses and `IrSource::is_empty` is
//! two, and neither had a test — this crate had no test module. Every clause
//! could be dropped on its own with the whole workspace green, and the effect
//! of dropping one is silent: `into_option` returns `None`, the exporter writes
//! `imessage: null`, and that field is gone from the export with nothing said.
//! A tapback, a reply thread, a send effect — each is one clause.

use super::*;

/// Set exactly one field on an otherwise default `IrImessage`, and name it.
fn one_field_set() -> Vec<(&'static str, IrImessage)> {
    let json = || Some(serde_json::json!([{"index": 0}]));
    vec![
        (
            "is_reply",
            IrImessage {
                is_reply: true,
                ..IrImessage::default()
            },
        ),
        (
            "in_reply_to_guid",
            IrImessage {
                in_reply_to_guid: Some("parent-guid".into()),
                ..IrImessage::default()
            },
        ),
        (
            "thread_originator_part",
            IrImessage {
                thread_originator_part: Some(0),
                ..IrImessage::default()
            },
        ),
        (
            "num_replies",
            IrImessage {
                num_replies: Some(0),
                ..IrImessage::default()
            },
        ),
        (
            "is_deleted",
            IrImessage {
                is_deleted: true,
                ..IrImessage::default()
            },
        ),
        (
            "send_effect",
            IrImessage {
                send_effect: Some("slam".into()),
                ..IrImessage::default()
            },
        ),
        (
            "shared_location",
            IrImessage {
                shared_location: Some("shared".into()),
                ..IrImessage::default()
            },
        ),
        (
            "announcement",
            IrImessage {
                announcement: Some("named the conversation".into()),
                ..IrImessage::default()
            },
        ),
        (
            "read_receipt_rfc3339",
            IrImessage {
                read_receipt_rfc3339: Some("2015-03-12T18:04:22Z".into()),
                ..IrImessage::default()
            },
        ),
        (
            "parts",
            IrImessage {
                parts: json(),
                ..IrImessage::default()
            },
        ),
        (
            "edits",
            IrImessage {
                edits: json(),
                ..IrImessage::default()
            },
        ),
        (
            "tapbacks",
            IrImessage {
                tapbacks: json(),
                ..IrImessage::default()
            },
        ),
        (
            "app",
            IrImessage {
                app: json(),
                ..IrImessage::default()
            },
        ),
        (
            "balloon_bundle_id",
            IrImessage {
                balloon_bundle_id: Some("com.apple.messages.URLBalloonProvider".into()),
                ..IrImessage::default()
            },
        ),
        (
            "balloon_kind",
            IrImessage {
                balloon_kind: Some("url".into()),
                ..IrImessage::default()
            },
        ),
        (
            "associated_guid",
            IrImessage {
                associated_guid: Some("assoc-guid".into()),
                ..IrImessage::default()
            },
        ),
        (
            "associated_part",
            IrImessage {
                associated_part: Some(0),
                ..IrImessage::default()
            },
        ),
        (
            "tapback_kind",
            IrImessage {
                tapback_kind: Some("loved".into()),
                ..IrImessage::default()
            },
        ),
        (
            "tapback_emoji",
            IrImessage {
                tapback_emoji: Some("\u{2764}".into()),
                ..IrImessage::default()
            },
        ),
        (
            "tapback_action",
            IrImessage {
                tapback_action: Some("added".into()),
                ..IrImessage::default()
            },
        ),
    ]
}

/// A default `IrImessage` carries nothing, so the exporter writes no
/// `imessage` object rather than an object of twenty nulls.
#[test]
fn an_imessage_with_nothing_set_is_empty() {
    let blank = IrImessage::default();
    assert!(blank.is_empty());
    assert!(blank.into_option().is_none());
}

/// Every field on its own is enough to keep the object.
///
/// A dropped clause loses exactly one kind of Apple extension from every
/// export, and nothing else changes — which is why this is one case per field
/// rather than one case with everything set.
#[test]
fn any_one_imessage_field_makes_it_worth_keeping() {
    let cases = one_field_set();
    assert_eq!(
        cases.len(),
        20,
        "one case per field on IrImessage; add one when a field is added"
    );
    for (field, value) in cases {
        assert!(
            !value.is_empty(),
            "{field} set on its own must not read as empty"
        );
        assert!(
            value.into_option().is_some(),
            "{field} set on its own must survive into_option"
        );
    }
}

/// `IrSource` is the same shape with two clauses: the Android type code and
/// the bag of vendor fields. Losing either drops the vendor leftovers that let
/// a reader see what the source actually said.
#[test]
fn a_source_is_empty_only_when_it_has_neither_a_type_nor_a_field() {
    let blank = IrSource::default();
    assert!(blank.is_empty());
    assert!(blank.into_option().is_none());

    let typed = IrSource {
        android_type: Some(1),
        ..IrSource::default()
    };
    assert!(
        !typed.is_empty(),
        "an android type is a leftover worth keeping"
    );
    assert!(typed.into_option().is_some());

    let mut fields = serde_json::Map::new();
    fields.insert("read".into(), serde_json::json!("1"));
    let with_fields = IrSource {
        fields,
        ..IrSource::default()
    };
    assert!(!with_fields.is_empty(), "a vendor field is worth keeping");
    assert!(with_fields.into_option().is_some());

    // An empty map is not a field.
    let empty_map = IrSource {
        fields: serde_json::Map::new(),
        ..IrSource::default()
    };
    assert!(empty_map.is_empty());
}
