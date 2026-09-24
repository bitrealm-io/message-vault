//! Body parse + GUID-based attachment index resolution.

use std::collections::{HashMap, HashSet};

use imessage_database::tables::{
    attachment::Attachment,
    messages::{
        Message,
        models::{AttributedRange, BubbleComponent},
    },
};
use rusqlite::Connection;

/// Apply typedstream body when parse succeeds (fills `components` / text).
pub(crate) fn apply_body(msg: &mut Message, db: &Connection) {
    if let Ok(body) = msg.parse_body(db) {
        msg.apply_body(body);
    }
}

pub(crate) struct AttachmentResolver {
    by_guid: HashMap<String, usize>,
    /// Attachment indices that have a GUID (prefer GUID matching for these).
    has_guid: HashSet<usize>,
    claimed: HashSet<usize>,
    len: usize,
}

impl AttachmentResolver {
    /// Index attachments by GUID so body ranges can resolve them.
    pub(crate) fn new(attachments: &[Attachment]) -> Self {
        let mut by_guid = HashMap::new();
        let mut has_guid = HashSet::new();
        for (i, a) in attachments.iter().enumerate() {
            if let Some(g) = a.guid.clone() {
                by_guid.insert(g, i);
                has_guid.insert(i);
            }
        }
        Self {
            by_guid,
            has_guid,
            claimed: HashSet::new(),
            len: attachments.len(),
        }
    }

    /// Pick the attachment index for this body range (GUID match, else next unused).
    pub(crate) fn resolve(&mut self, range: &AttributedRange) -> usize {
        if let Some(idx) = range
            .attachment
            .as_ref()
            .and_then(|meta| meta.guid.as_deref())
            .and_then(|guid| self.by_guid.get(guid).copied())
        {
            self.claimed.insert(idx);
            return idx;
        }
        // Take the first unclaimed row, preferring rows that have no GUID,
        // which are the ones the positional fallback is meant for. A claimed
        // row is never reused; with none left the index runs past the rows,
        // and callers drop it.
        let unclaimed = |i: &usize| !self.claimed.contains(i);
        let idx = (0..self.len)
            .filter(unclaimed)
            .find(|i| !self.has_guid.contains(i))
            .or_else(|| (0..self.len).find(unclaimed))
            .unwrap_or(self.len);
        self.claimed.insert(idx);
        idx
    }
}

/// Pair each attributed range with the attachment index it refers to, if any.
pub(crate) fn resolve_run<'r>(
    ranges: &'r [AttributedRange],
    resolver: &mut AttachmentResolver,
) -> Vec<(&'r AttributedRange, Option<usize>)> {
    ranges
        .iter()
        .map(|range| {
            let idx = range.attachment.is_some().then(|| resolver.resolve(range));
            (range, idx)
        })
        .collect()
}

/// Indices into `attachments` referenced by the message body.
///
/// When `components` is empty (parse failure), falls back to every join row.
pub(crate) fn referenced_attachment_indices(
    message: &Message,
    attachments: &[Attachment],
) -> Vec<usize> {
    if attachments.is_empty() {
        return Vec::new();
    }

    if message.components.is_empty() {
        return (0..attachments.len()).collect();
    }

    let mut resolver = AttachmentResolver::new(attachments);
    let mut indices = HashSet::new();

    for (part_idx, component) in message.components.iter().enumerate() {
        match component {
            BubbleComponent::Run(ranges) => {
                if message.is_part_edited(part_idx) {
                    continue;
                }
                for (_, idx) in resolve_run(ranges, &mut resolver) {
                    if let Some(i) = idx
                        && i < attachments.len()
                    {
                        indices.insert(i);
                    }
                }
            }
            BubbleComponent::App | BubbleComponent::Retracted => {}
        }
    }

    let mut out: Vec<_> = indices.into_iter().collect();
    out.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use imessage_database::tables::messages::models::AttachmentMeta;

    fn stub_attachment(guid: Option<&str>) -> Attachment {
        Attachment {
            rowid: 0,
            guid: guid.map(str::to_string),
            filename: None,
            uti: None,
            mime_type: None,
            transfer_name: None,
            total_bytes: 0,
            is_sticker: false,
            hide_attachment: 0,
            emoji_description: None,
            copied_path: None,
        }
    }

    fn att_range(guid: Option<&str>) -> AttributedRange {
        AttributedRange::attachment(
            0,
            1,
            AttachmentMeta {
                guid: guid.map(str::to_string),
                ..AttachmentMeta::default()
            },
        )
    }

    #[test]
    fn positional_skips_guid_claimed_indices() {
        let attachments = vec![
            stub_attachment(Some("guid-a")),
            stub_attachment(Some("guid-b")),
            stub_attachment(None),
        ];
        let mut resolver = AttachmentResolver::new(&attachments);
        // First body range points at attachment 0 by GUID.
        assert_eq!(resolver.resolve(&att_range(Some("guid-a"))), 0);
        // Second range has no usable GUID — must take the next free slot (2),
        // not reuse 0.
        assert_eq!(resolver.resolve(&att_range(None)), 2);
        // Third range resolves the remaining GUID attachment.
        assert_eq!(resolver.resolve(&att_range(Some("guid-b"))), 1);
    }

    /// With every GUID-less row taken, a range with no GUID falls back to
    /// the next row nobody has claimed, stepping over the ones already
    /// matched by GUID.
    #[test]
    fn positional_takes_the_next_unclaimed_row_when_every_row_has_a_guid() {
        let attachments = vec![
            stub_attachment(Some("guid-a")),
            stub_attachment(Some("guid-b")),
            stub_attachment(Some("guid-c")),
        ];
        let mut resolver = AttachmentResolver::new(&attachments);
        assert_eq!(resolver.resolve(&att_range(Some("guid-a"))), 0);
        assert_eq!(resolver.resolve(&att_range(None)), 1);
        assert_eq!(resolver.resolve(&att_range(Some("guid-c"))), 2);
        // Nothing is left, so the index runs past the rows.
        assert_eq!(resolver.resolve(&att_range(None)), 3);
        assert_eq!(resolver.resolve(&att_range(None)), 3);
    }

    /// A row with a GUID that no range names is still the message's, so a
    /// range with no GUID takes it once the GUID-less rows are gone, even
    /// when it comes before them.
    #[test]
    fn positional_takes_an_earlier_unclaimed_guid_row_before_running_out() {
        let attachments = vec![stub_attachment(Some("guid-a")), stub_attachment(None)];
        let mut resolver = AttachmentResolver::new(&attachments);
        assert_eq!(resolver.resolve(&att_range(None)), 1);
        assert_eq!(resolver.resolve(&att_range(None)), 0);
    }

    /// A message body whose ranges refer to one attachment by GUID and one
    /// by position keeps both, and a body with more attachment ranges than
    /// the message has rows keeps only the rows that exist.
    #[test]
    fn referenced_indices_keep_every_row_the_body_refers_to_and_no_more() {
        let fixture = crate::test_support::FixtureDb::write();
        let session = fixture.session();
        let mut message = crate::test_support::FixtureDb::messages(&session).remove(1);
        let attachments = vec![stub_attachment(None), stub_attachment(Some("guid-b"))];

        message.components = vec![BubbleComponent::Run(vec![
            att_range(Some("guid-b")),
            att_range(None),
        ])];
        assert_eq!(
            referenced_attachment_indices(&message, &attachments),
            vec![0, 1]
        );

        message.components = vec![BubbleComponent::Run(vec![
            att_range(None),
            att_range(None),
            att_range(None),
        ])];
        assert_eq!(
            referenced_attachment_indices(&message, &attachments),
            vec![0, 1],
            "the third range has no row to point at"
        );

        // A body that never parsed refers to every row.
        message.components = Vec::new();
        assert_eq!(
            referenced_attachment_indices(&message, &attachments),
            vec![0, 1]
        );
        assert!(referenced_attachment_indices(&message, &[]).is_empty());
    }
}
