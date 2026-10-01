-- Keep messages_fts in step with messages and attachments. One index row
-- holds a message's body, subject, and the names and transcriptions of all
-- its attachments, so every change removes the message's row and writes it
-- again whole. messages_fts is a contentless-delete table (fts_virtual.sql):
-- DELETE by rowid removes every term the row indexed, attachment terms
-- included, without being told what they were.
CREATE TRIGGER messages_fts_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    VALUES (
        new.id,
        coalesce(new.body, ''),
        coalesce(new.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(original_name, '') || ' ' || coalesce(transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments
            WHERE message_id = new.id
        )
    );
END;

CREATE TRIGGER messages_fts_ad AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
END;

CREATE TRIGGER messages_fts_au AFTER UPDATE OF body, subject ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    VALUES (
        new.id,
        coalesce(new.body, ''),
        coalesce(new.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(original_name, '') || ' ' || coalesce(transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments
            WHERE message_id = new.id
        )
    );
END;

CREATE TRIGGER attachments_fts_ai AFTER INSERT ON attachments BEGIN
    DELETE FROM messages_fts WHERE rowid = new.message_id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        m.id,
        coalesce(m.body, ''),
        coalesce(m.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments a
            WHERE a.message_id = m.id
        )
    FROM messages m WHERE m.id = new.message_id;
END;

CREATE TRIGGER attachments_fts_ad AFTER DELETE ON attachments BEGIN
    DELETE FROM messages_fts WHERE rowid = old.message_id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        m.id,
        coalesce(m.body, ''),
        coalesce(m.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments a
            WHERE a.message_id = m.id
        )
    FROM messages m WHERE m.id = old.message_id;
END;

CREATE TRIGGER attachments_fts_au AFTER UPDATE OF original_name, transcription ON attachments BEGIN
    DELETE FROM messages_fts WHERE rowid = new.message_id;
    INSERT INTO messages_fts(rowid, body, subject, attachment_text)
    SELECT
        m.id,
        coalesce(m.body, ''),
        coalesce(m.subject, ''),
        (
            SELECT coalesce(
                group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                ),
                ''
            )
            FROM attachments a
            WHERE a.message_id = m.id
        )
    FROM messages m WHERE m.id = new.message_id;
END;
