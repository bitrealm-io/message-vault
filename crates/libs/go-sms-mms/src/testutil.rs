//! Build MMS PDUs the way a phone writes them, so a test's input is the
//! shape a backup holds rather than bytes typed by hand.
//!
//! The encoding follows the real GO SMS Pro backup byte for byte: a
//! received message is `m-retrieve-conf` with From, To, Message-Class,
//! Message-ID, Priority and Delivery-Report before Content-Type; a sent
//! message is `m-send-req` with the Insert-address-token as From. The body
//! is `application/vnd.wap.multipart.related` with a SMIL part first, then
//! a `text/plain; charset=utf-8` part named `text_0.txt` and any media.

/// One part to add to a built PDU.
#[derive(Debug, Clone)]
pub struct BuildPart {
    /// The media type written to the part's Content-Type.
    pub content_type: &'static str,
    /// The Name parameter, Content-ID and Content-Location, when set.
    pub name: Option<&'static str>,
    /// Whether to write a `charset=utf-8` parameter.
    pub charset_utf8: bool,
    /// The part's bytes.
    pub data: Vec<u8>,
}

impl BuildPart {
    /// A `text/plain; charset=utf-8` part named `text_0.txt`.
    pub fn text(body: &str) -> Self {
        Self {
            content_type: "text/plain",
            name: Some("text_0.txt"),
            charset_utf8: true,
            data: body.as_bytes().to_vec(),
        }
    }

    /// A JPEG part: the JFIF magic and `len` bytes of filler.
    pub fn jpeg(name: &'static str, len: usize) -> Self {
        let mut data = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        data.resize(len.max(data.len()), 0x11);
        Self {
            content_type: "image/jpeg",
            name: Some(name),
            charset_utf8: false,
            data,
        }
    }

    /// The SMIL layout a phone puts first in every MMS.
    pub fn smil() -> Self {
        Self {
            content_type: "application/smil",
            name: Some("smil.xml"),
            charset_utf8: false,
            data: b"<smil><head><layout><root-layout/></layout></head><body><par dur=\"5000ms\"><text src=\"text_0.txt\"/></par></body></smil>".to_vec(),
        }
    }
}

/// A PDU under construction.
#[derive(Debug, Clone)]
pub struct PduBuilder {
    sent: bool,
    from: Option<String>,
    to: Vec<String>,
    cc: Vec<String>,
    date: Option<u64>,
    subject: Option<String>,
    parts: Vec<BuildPart>,
    /// Raw header bytes inserted just before Content-Type, for tests that
    /// need a header the builder has no method for.
    extra_headers: Vec<u8>,
}

impl PduBuilder {
    /// An `m-retrieve-conf` from `from`.
    pub fn received(from: &str) -> Self {
        Self {
            sent: false,
            from: Some(from.to_string()),
            to: Vec::new(),
            cc: Vec::new(),
            date: Some(1_609_459_200),
            subject: None,
            parts: vec![BuildPart::smil()],
            extra_headers: Vec::new(),
        }
    }

    /// An `m-send-req` with the Insert-address-token as From.
    pub fn sent() -> Self {
        Self {
            sent: true,
            from: None,
            ..Self::received("")
        }
    }

    /// Add a To address.
    pub fn to(mut self, addr: &str) -> Self {
        self.to.push(addr.to_string());
        self
    }

    /// Add a Cc address.
    pub fn cc(mut self, addr: &str) -> Self {
        self.cc.push(addr.to_string());
        self
    }

    /// Set the Date header, or leave it out with `None`.
    pub fn date(mut self, secs: Option<u64>) -> Self {
        self.date = secs;
        self
    }

    /// Set the Subject header.
    pub fn subject(mut self, s: &str) -> Self {
        self.subject = Some(s.to_string());
        self
    }

    /// Append a body part.
    pub fn part(mut self, part: BuildPart) -> Self {
        self.parts.push(part);
        self
    }

    /// Append a `text/plain` part with `body`.
    pub fn text(self, body: &str) -> Self {
        self.part(BuildPart::text(body))
    }

    /// Start with no parts at all, not even the SMIL.
    pub fn no_parts(mut self) -> Self {
        self.parts.clear();
        self
    }

    /// Insert raw header bytes just before Content-Type.
    pub fn raw_header(mut self, bytes: &[u8]) -> Self {
        self.extra_headers.extend_from_slice(bytes);
        self
    }

    /// The PDU bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut out = vec![0x8c, if self.sent { 0x80 } else { 0x84 }];
        out.push(0x98);
        out.extend_from_slice(b"T1\0");
        out.extend_from_slice(&[0x8d, 0x90]);
        if let Some(d) = self.date {
            out.push(0x85);
            out.push(4);
            out.extend_from_slice(&(d as u32).to_be_bytes());
        }
        out.push(0x89);
        match &self.from {
            Some(f) => {
                let enc = utf8_string(&phone_address(f));
                out.push((1 + enc.len()) as u8);
                out.push(0x80);
                out.extend_from_slice(&enc);
            }
            None => out.extend_from_slice(&[0x01, 0x81]),
        }
        for t in &self.to {
            out.push(0x97);
            out.extend_from_slice(&utf8_string(&phone_address(t)));
        }
        for c in &self.cc {
            out.push(0x82);
            out.extend_from_slice(&utf8_string(&phone_address(c)));
        }
        if let Some(s) = &self.subject {
            out.push(0x96);
            out.extend_from_slice(&utf8_string(s));
        }
        out.extend_from_slice(&[0x8a, 0x80]); // Message-Class Personal
        out.push(0x8b);
        out.extend_from_slice(b"MSG-1\0"); // Message-ID
        out.extend_from_slice(&[0x8f, 0x81]); // Priority Normal
        out.extend_from_slice(&[0x86, 0x81]); // Delivery-Report no
        out.extend_from_slice(&self.extra_headers);
        // Content-Type: general form, multipart.related, Start <smil.xml>, Type application/smil
        let mut ct = vec![0xb3];
        ct.extend_from_slice(b"\x8a<smil.xml>\0");
        ct.extend_from_slice(b"\x89application/smil\0");
        out.push(0x84);
        value_length(&mut out, ct.len());
        out.extend_from_slice(&ct);
        uintvar(&mut out, self.parts.len() as u64);
        for p in &self.parts {
            let headers = part_headers(p);
            uintvar(&mut out, headers.len() as u64);
            uintvar(&mut out, p.data.len() as u64);
            out.extend_from_slice(&headers);
            out.extend_from_slice(&p.data);
        }
        out
    }
}

/// `+digits/TYPE=PLMN` unless the address already has a `/TYPE=` or `@`.
fn phone_address(a: &str) -> String {
    if a.contains("/TYPE=") || a.contains('@') {
        a.to_string()
    } else {
        format!("{a}/TYPE=PLMN")
    }
}

/// Encoded-string-value with the UTF-8 charset: Value-length, 0xea, text, NUL.
fn utf8_string(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    value_length(&mut v, 1 + s.len() + 1);
    v.push(0xea);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

/// Value-length: one byte up to 30, else 31 and a uintvar.
fn value_length(out: &mut Vec<u8>, n: usize) {
    if n <= 30 {
        out.push(n as u8);
    } else {
        out.push(31);
        uintvar(out, n as u64);
    }
}

fn uintvar(out: &mut Vec<u8>, mut v: u64) {
    let mut bytes = vec![(v & 0x7f) as u8];
    v >>= 7;
    while v > 0 {
        bytes.push(((v & 0x7f) as u8) | 0x80);
        v >>= 7;
    }
    bytes.reverse();
    out.extend_from_slice(&bytes);
}

/// Part headers as a phone writes them: Content-Type in general form with a
/// Name parameter (and Charset for text), then Content-ID and
/// Content-Location naming the same file.
fn part_headers(p: &BuildPart) -> Vec<u8> {
    let mut ct = Vec::new();
    match p.content_type {
        "text/plain" => ct.push(0x83),
        "image/jpeg" => ct.push(0x9e),
        "image/gif" => ct.push(0x9d),
        "image/png" => ct.push(0xa0),
        _ => {
            ct.extend_from_slice(p.content_type.as_bytes());
            ct.push(0);
        }
    }
    if let Some(n) = p.name {
        ct.push(0x85);
        ct.extend_from_slice(n.as_bytes());
        ct.push(0);
    }
    if p.charset_utf8 {
        ct.extend_from_slice(&[0x81, 0xea]);
    }
    let mut h = Vec::new();
    value_length(&mut h, ct.len());
    h.extend_from_slice(&ct);
    if let Some(n) = p.name {
        h.push(0xc0);
        h.push(b'"');
        h.extend_from_slice(format!("<{n}>").as_bytes());
        h.push(0);
        h.push(0x8e);
        h.extend_from_slice(n.as_bytes());
        h.push(0);
    }
    h
}
