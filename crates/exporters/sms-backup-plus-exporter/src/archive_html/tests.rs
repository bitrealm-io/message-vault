use super::*;

/// The bubble markup the archive generator writes, one message.
fn bubble(align: &str, colour: &str, stamp: &str, sender: &str, text: &str) -> String {
    format!(
        "<div style='text-align:{align};color:#999;font-size:0.75em;margin:5px 10px;'>{stamp} - {sender}</div>\n\
         <div style='text-align:{align};margin:2px 10px;'><div style='background-color:{colour};color:#000;padding:8px 12px;'>\
         <p style='margin:0;'>{text}</p></div></div>"
    )
}

#[test]
fn each_bubble_becomes_a_timestamp_line_and_its_text() {
    let html = format!(
        "<html><body>{}\n{}</body></html>",
        bubble(
            "right",
            "#0b93f6",
            "2019-01-26 21:35:29",
            "Me",
            "Thanks for the invite!"
        ),
        bubble(
            "left",
            "#e5e5ea",
            "2019-01-27 00:01:21",
            "Alice",
            "Join us whenever."
        ),
    );
    // The blank line between a timestamp and its text is what the plain-text
    // half of the same mail writes too, so both halves parse alike.
    assert_eq!(
        transcript_from_html(&html),
        "2019-01-26 21:35:29 - Me\n\
         \n\
         Thanks for the invite!\n\
         \n\
         2019-01-27 00:01:21 - Alice\n\
         \n\
         Join us whenever."
    );
}

#[test]
fn a_stray_less_than_stays_in_the_message() {
    // The generator does not escape message text. A pattern that treated every
    // `<` as a tag would eat `<3` and everything up to the next `>`, silently
    // deleting words from the middle of a conversation.
    let html = "<p>i love it &lt;3 and &lt;$120 too</p><p>plain <3 and <$120 too</p>";
    let out = transcript_from_html(html);
    assert!(out.contains("i love it <3 and <$120 too"), "{out:?}");
    assert!(out.contains("plain <3 and <$120 too"), "{out:?}");
}

#[test]
fn entities_are_decoded() {
    assert_eq!(
        transcript_from_html("<p>Tom &amp; Jerry &quot;quoted&quot; &#39;apostrophe&#39;</p>"),
        "Tom & Jerry \"quoted\" 'apostrophe'"
    );
}

#[test]
fn runs_of_blank_lines_collapse_to_one() {
    // Nested closing tags each contribute a line break; the plain-text part of
    // the same mail separates these two paragraphs with a single blank line.
    let html = "<div><p>first</p></div>\n\n\n<div><p>second</p></div>";
    assert_eq!(transcript_from_html(html), "first\n\nsecond");
}

#[test]
fn an_unclosed_tag_at_the_end_does_not_panic() {
    assert_eq!(transcript_from_html("<p>kept</p><div style='x"), "kept");
}

#[test]
fn multibyte_text_survives_stripping() {
    // `<` and `>` are ASCII, but the text between them need not be; slicing on
    // a byte index would panic if it landed mid-character.
    assert_eq!(
        transcript_from_html("<p>中文名 café 😀</p>"),
        "中文名 café 😀"
    );
}

#[test]
fn text_with_no_markup_is_returned_as_is() {
    assert_eq!(transcript_from_html("just words"), "just words");
}
