-- Contentless FTS5 index over message body/subject plus attachment text.
-- contentless_delete=1 lets a row be deleted by rowid alone. Without it a
-- delete has to repeat the exact text that was indexed, and any term it
-- leaves out stays in the index under that rowid, where the next message
-- given the same id is found by it.
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    -- Indexed message body text (synced from messages.body).
    body,
    -- Indexed message subject text (synced from messages.subject).
    subject,
    -- Indexed attachment filenames + transcriptions for the message.
    attachment_text,
    content='',
    contentless_delete=1,
    tokenize='unicode61 remove_diacritics 2'
);
