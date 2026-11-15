//! An interchange's segments, one at a time, as the Foundation's one segment
//! walk (`message::segment`) cuts them in the service characters in force:
//! each tag held to three capital letters or digits, and a segment cut into
//! its elements and components only when asked, every release taken out.
//! What [`Interchange::walk`](crate::syntax::Interchange::walk) yields, and
//! what [`Interchange::parse`](crate::syntax::Interchange::parse) collects.

use std::borrow::Cow;

use contract::ValidationIssue;
// The segment is the capability's: EDIFACT and X12 read the same shape
// (ADR-0044); the syntax that cuts it out of an interchange is this crate's.
use contract::segment::Segment;
use message::segment::piece::Piece;
use message::segment::{Delimiters, Walk};

/// An interchange's segments, one at a time, in the order they lie, as
/// [`Interchange::walk`](crate::syntax::Interchange::walk) begins them.
/// After a refusal it yields nothing.
#[derive(Clone, Debug)]
pub struct Segments<'a> {
    advice: Option<&'a str>,
    delimiters: Delimiters,
    /// The text after the `UNA`, which is walked.
    body: &'a str,
    /// None once the walk has ended.
    walk: Option<Walk<'a>>,
    /// Whether the walk has met a segment.
    opened: bool,
}

impl<'a> Segments<'a> {
    /// `body`, the text after the `UNA` where there is one, walked with
    /// `delimiters`, the service characters in force.
    pub(crate) const fn new(
        advice: Option<&'a str>,
        delimiters: Delimiters,
        body: &'a str,
    ) -> Self {
        Self {
            advice,
            delimiters,
            body,
            walk: Some(delimiters.walk(body.as_bytes())),
            opened: false,
        }
    }

    /// The `UNA` advice as it was written, where there was one.
    #[must_use]
    pub const fn advice(&self) -> Option<&'a str> {
        self.advice
    }

    /// The service characters in force, as the walk cuts by them.
    #[must_use]
    pub const fn delimiters(&self) -> Delimiters {
        self.delimiters
    }
}

impl<'a> Iterator for Segments<'a> {
    type Item = Result<Lying<'a>, ValidationIssue>;

    fn next(&mut self) -> Option<Self::Item> {
        let walked = self.walk.as_mut()?.next()?;
        let lying = walked
            .map_err(|(reason, _)| ValidationIssue::malformed(reason))
            .and_then(|walked| {
                // An interchange opens with its first segment (ISO 9735):
                // what stands before it is read as that segment's tag.
                if !self.opened && walked.offset > 0 {
                    let head = &self.body[..walked.offset + walked.tag.len()];
                    let refused = format!("{head:?} is not a segment tag");
                    return Err(ValidationIssue::malformed(refused));
                }
                Lying::cut(walked, self.body, self.delimiters)
            });
        self.opened = true;
        if lying.is_err() {
            self.walk = None;
        }
        Some(lying)
    }
}

/// One segment as it lies in the interchange: its tag, and its text from
/// the tag to its terminator with every release in place.
#[derive(Clone, Debug)]
pub struct Lying<'a> {
    tag: Cow<'a, str>,
    text: &'a str,
    walked: message::segment::Segment<'a>,
    delimiters: Delimiters,
}

impl<'a> Lying<'a> {
    /// `walked`, a segment of `body`, its tag — the first component of its
    /// first element, a released character read through — held to three
    /// capital letters or digits.
    fn cut(
        walked: message::segment::Segment<'a>,
        body: &'a str,
        delimiters: Delimiters,
    ) -> Result<Self, ValidationIssue> {
        let mut lying = Self {
            tag: Cow::Borrowed(""),
            text: &body[walked.offset..walked.offset + walked.bytes.len()],
            walked,
            delimiters,
        };
        if let Some(head) = walked.pieces(&delimiters).next() {
            lying.tag = lying.value(head);
        }
        let tag = &lying.tag;
        let sound = tag.len() == 3
            && tag
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit());
        if !sound {
            return Err(ValidationIssue::malformed(format!(
                "{tag:?} is not a segment tag"
            )));
        }
        Ok(lying)
    }

    /// The segment's tag: `UNH`, `BGM`, `NAD`.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The segment, cut into its elements and their components, every
    /// release taken out.
    #[must_use]
    pub fn segment(&self) -> Segment {
        let mut elements: Vec<Vec<String>> = Vec::new();
        let pieces = self.walked.pieces(&self.delimiters);
        // The tag's own element is the tag, as it was read.
        for piece in pieces.skip_while(|piece| piece.element == 0) {
            let next = piece.component > 0;
            let value = self.value(piece).into_owned();
            match elements.last_mut() {
                Some(element) if next => element.push(value),
                _ => elements.push(vec![value]),
            }
        }
        Segment {
            tag: self.tag.to_string(),
            elements,
        }
    }

    /// What `piece` of this segment holds, every release taken out: a
    /// component no release stands in is borrowed whole from the text.
    fn value(&self, piece: Piece) -> Cow<'a, str> {
        let text: &'a str = self.text;
        if piece.released {
            let unreleased = self.delimiters.unreleased(&self.walked.bytes[piece.at]);
            Cow::Owned(String::from_utf8_lossy_owned(unreleased))
        } else {
            Cow::Borrowed(&text[piece.at])
        }
    }
}
