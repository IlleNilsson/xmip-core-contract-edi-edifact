//! ISO 9735 syntax: an interchange walked a segment at a time in its own
//! service characters, read into segments, written back in them, and the
//! soundness checks the service segments carry.
//!
//! The walk is the Foundation's one segment walk (`message::segment`),
//! given the service characters the `UNA` advises or the default set, as
//! the EDIFACT shape's one reader of the `UNA` reads them (`shape::advice`):
//! a segment ends at the first terminator no release stands before, the
//! whitespace between two segments is skipped, and the interchange opens
//! with its first segment, as ISO 9735 has it. What reads one segment cuts
//! only that one and walks the rest uncut ([`Interchange::walk`], its
//! [`Segments`]), and [`Interchange::parse`] cuts every one into the tree a
//! write edits: one walk, so a segment is cut, a release honored and a
//! malformed interchange refused in the same words by both.
//!
//! An issue's `code` is `malformed` when the text cannot be read as segments at
//! all, else `envelope` for a service-segment departure; its `path` is
//! `segment N (TAG)`.

use contract::ValidationIssue;
// The segment is the capability's: EDIFACT and X12 read the same shape
// (ADR-0044); the syntax that cuts it out of an interchange is this crate's.
use contract::segment::Segment;
use message::segment::Delimiters;

use crate::segments::Segments;

/// A read interchange.
pub struct Interchange {
    /// The `UNA` advice as it was written, where there was one.
    advice: Option<String>,
    delimiters: Delimiters,
    segments: Vec<Segment>,
}

impl Interchange {
    /// `text`'s segments, one at a time, read with the service characters
    /// its `UNA` advises, else the default set.
    ///
    /// # Errors
    /// The `malformed` issue in the shape's words when the `UNA` cannot be
    /// read: shorter than its nine characters, or with a service character
    /// outside ASCII.
    pub fn walk(text: &str) -> Result<Segments<'_>, ValidationIssue> {
        let advice = shape::advice(text.as_bytes())
            .map_err(|refused| ValidationIssue::malformed(refused.reason))?;
        // The UNA is ASCII, as the shape holds it, so the body starts on a
        // character of the text.
        let (una, body) = text.split_at(advice.una.map_or(0, <[u8]>::len));
        let una = Some(una).filter(|una| !una.is_empty());
        Ok(Segments::new(una, advice.delimiters, body))
    }

    /// Read `text` with its own service characters.
    ///
    /// # Errors
    /// The one `malformed` issue when the text is not segments at all.
    pub fn parse(text: &str) -> Result<Self, ValidationIssue> {
        let walk = Self::walk(text)?;
        let advice = walk.advice().map(str::to_string);
        let delimiters = walk.delimiters();
        let segments = walk
            .map(|lying| lying.map(|lying| lying.segment()))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            advice,
            delimiters,
            segments,
        })
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The segments, to change one before the interchange is
    /// [`written`](Self::written) again.
    pub fn segments_mut(&mut self) -> &mut [Segment] {
        &mut self.segments
    }

    /// The service characters in force, as the walk cuts by them.
    #[must_use]
    pub const fn delimiters(&self) -> Delimiters {
        self.delimiters
    }

    /// The interchange written with its own service characters, its `UNA`
    /// as it was, every service character in a value released.
    ///
    /// # Errors
    /// A value carries a service character where the `UNA` advises no
    /// release character, so it cannot be written.
    pub fn written(&self) -> Result<String, String> {
        // Every service character is ASCII, as the shape reads the UNA.
        let d = self.delimiters;
        let [component, data, terminator] = [d.composite, d.element, d.terminator].map(char::from);
        let release = d.release.map(char::from);
        let mut text = self.advice.clone().unwrap_or_default();
        for (number, segment) in self.segments.iter().enumerate() {
            text.push_str(&segment.tag);
            for element in &segment.elements {
                text.push(data);
                for (index, value) in element.iter().enumerate() {
                    if index > 0 {
                        text.push(component);
                    }
                    for ch in value.chars() {
                        let service = [component, data, terminator].contains(&ch);
                        if service || Some(ch) == release {
                            let Some(release) = release else {
                                return Err(format!(
                                    "segment {} ({}) holds {value:?}, which carries a \
                                     service character, and the UNA advises no release character",
                                    number + 1,
                                    segment.tag
                                ));
                            };
                            text.push(release);
                        }
                        text.push(ch);
                    }
                }
            }
            text.push(terminator);
        }
        Ok(text)
    }

    /// Every `UNH`, numbered from 1.
    pub fn message_headers(&self) -> impl Iterator<Item = (usize, &Segment)> {
        self.segments
            .iter()
            .filter(|s| s.tag == "UNH")
            .enumerate()
            .map(|(i, s)| (i + 1, s))
    }

    /// Every departure of the service segments from ISO 9735.
    #[must_use]
    pub fn soundness(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let at = |n: usize| format!("segment {} ({})", n + 1, self.segments[n].tag);
        let envelope = |message: &str, n: usize| ValidationIssue::at("envelope", message, at(n));
        let first = &self.segments[0];
        if first.tag != "UNB" {
            issues.push(envelope("the interchange does not open with UNB", 0));
        }
        let last = self.segments.len() - 1;
        if self.segments[last].tag != "UNZ" {
            issues.push(envelope("the interchange does not close with UNZ", last));
        }
        let mut open_message: Option<(usize, String)> = None;
        let mut open_group: Option<(usize, String, usize)> = None;
        let mut messages = 0;
        for (n, segment) in self.segments.iter().enumerate() {
            match segment.tag.as_str() {
                "UNH" => {
                    if let Some((start, _)) = &open_message {
                        issues.push(envelope(
                            &format!("UNH inside the message opened at segment {}", start + 1),
                            n,
                        ));
                    }
                    open_message = Some((n, segment.simple(1).to_string()));
                    messages += 1;
                }
                "UNT" => match open_message.take() {
                    Some((start, reference)) => {
                        let counted = n - start + 1;
                        if segment.simple(1).parse::<usize>().ok() != Some(counted) {
                            let message = format!(
                                "UNT counts {} segments, the message has {counted}",
                                segment.simple(1)
                            );
                            issues.push(envelope(&message, n));
                        }
                        if segment.simple(2) != reference {
                            let message = format!(
                                "UNT closes {}, the UNH opened {reference}",
                                segment.simple(2)
                            );
                            issues.push(envelope(&message, n));
                        }
                    }
                    None => issues.push(envelope("UNT with no message open", n)),
                },
                "UNG" => {
                    open_group = Some((n, segment.simple(5).to_string(), messages));
                }
                "UNE" => match open_group.take() {
                    Some((_, reference, before)) => {
                        let in_group = messages - before;
                        if segment.simple(1).parse::<usize>().ok() != Some(in_group) {
                            let message = format!(
                                "UNE counts {} messages, the group has {in_group}",
                                segment.simple(1)
                            );
                            issues.push(envelope(&message, n));
                        }
                        if segment.simple(2) != reference {
                            issues.push(envelope("UNE does not close the reference UNG opened", n));
                        }
                    }
                    None => issues.push(envelope("UNE with no group open", n)),
                },
                "UNZ" => {
                    if segment.simple(1).parse::<usize>().ok() != Some(messages)
                        && open_group.is_none()
                    {
                        let message = format!(
                            "UNZ counts {}, the interchange has {messages} messages",
                            segment.simple(1)
                        );
                        issues.push(envelope(&message, n));
                    }
                    if first.tag == "UNB" && segment.simple(2) != first.simple(5) {
                        let message = format!(
                            "UNZ closes {}, UNB opened {}",
                            segment.simple(2),
                            first.simple(5)
                        );
                        issues.push(envelope(&message, n));
                    }
                }
                _ => {}
            }
        }
        if let Some((start, _)) = open_message {
            issues.push(envelope("the message is never closed by UNT", start));
        }
        if let Some((start, _, _)) = open_group {
            issues.push(envelope("the group is never closed by UNE", start));
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interchange_is_written_back_as_it_was_read() {
        let text = "UNA:+.? 'UNB+UNOC:3+S+R'FTX+AAI+++a?+b?'c'UNZ+0+R'";
        let mut read = Interchange::parse(text).expect("parses");
        assert_eq!(read.written().expect("written"), text);
        read.segments_mut()[1].elements[3][0] = "x:y".to_string();
        assert_eq!(
            read.written().expect("written"),
            "UNA:+.? 'UNB+UNOC:3+S+R'FTX+AAI+++x?:y'UNZ+0+R'"
        );
        let bare = Interchange::parse("UNB+UNOC:3+S+R'UNZ+0+R'").expect("parses");
        assert_eq!(bare.written().expect("written"), "UNB+UNOC:3+S+R'UNZ+0+R'");
    }

    #[test]
    fn una_sets_the_service_characters_and_release_escapes() {
        let text =
            "UNA|^,! ~UNB^UNOC|3^S^R^260907|1345^REF~FTX^AAI^^^Price is 5!^ per unit~UNZ^0^REF~";
        let read = Interchange::parse(text).expect("parses");
        assert_eq!(read.segments()[1].simple(4), "Price is 5^ per unit");
        assert_eq!(read.segments()[0].element(1), ["UNOC", "3"]);
    }

    /// A space in UNA4 is no release character (ISO 9735 syntax 3: *if not
    /// used, insert space character*): `?` is then a character like any.
    #[test]
    fn a_space_advised_as_the_release_character_is_none() {
        let text = "UNA:+.  'UNB+UNOC:3+S+R'FTX+AAI+++Why? Because'UNZ+0+R'";
        let mut read = Interchange::parse(text).expect("parses");
        assert_eq!(read.delimiters().release, None);
        // One reader of the UNA: the shape's, whatever reads the interchange.
        let advised = shape::advice(text.as_bytes()).expect("advised");
        assert_eq!(read.delimiters(), advised.delimiters);
        assert_eq!(read.segments()[1].simple(4), "Why? Because");
        assert_eq!(read.segments()[1].tag, "FTX");
        assert_eq!(read.written().expect("written"), text);
        // A space and a `?` in a value stay as they are, nothing released.
        read.segments_mut()[1].elements[3][0] = "a b?".to_string();
        assert!(read.written().expect("written").contains("FTX+AAI+++a b?'"));
        // A value holding a service character cannot be written.
        for held in ["a+b", "a:b", "a'b"] {
            read.segments_mut()[1].elements[3][0] = held.to_string();
            let refused = read.written().expect_err("no release character");
            assert!(refused.contains("segment 2 (FTX)"), "{refused}");
            assert!(
                refused.contains("advises no release character"),
                "{refused}"
            );
        }
        // With no release character, a `?` before the terminator ends nothing.
        let ended = Interchange::parse("UNA:+.  'UNB+X?'UNZ+0+R'").expect("parses");
        assert_eq!(ended.segments()[0].simple(1), "X?");
        assert_eq!(ended.segments().len(), 2);
    }

    #[test]
    fn counts_and_references_are_checked() {
        let text =
            "UNB+UNOC:3+S+R+260907:1345+REF1'UNH+1+ORDERS:D:96A:UN'BGM+220'UNT+2+1'UNZ+2+REF9'";
        let read = Interchange::parse(text).expect("parses");
        let issues = read.soundness();
        let messages: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.starts_with("UNT counts 2 segments, the message has 3")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.starts_with("UNZ counts 2")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.starts_with("UNZ closes REF9")),
            "{messages:?}"
        );
    }

    #[test]
    fn a_missing_terminator_and_a_bad_tag_are_malformed() {
        assert!(Interchange::parse("UNB+X").is_err());
        assert!(Interchange::parse("un+X'").is_err());
        assert!(Interchange::parse("UNA:+.?").is_err());
    }

    #[test]
    fn a_walk_yields_a_segment_at_a_time_and_cuts_one_only_when_asked() {
        let text = "UNA:+.? 'UNB+UNOC:3+S+R'\nFTX+AAI+++a?'b??:c?+d'UNZ+0+R'";
        let mut walk = Interchange::walk(text).expect("advised");
        assert_eq!(walk.advice(), Some("UNA:+.? '"));
        assert_eq!(walk.next().expect("one").expect("sound").tag(), "UNB");
        let ftx = walk.next().expect("two").expect("sound");
        assert_eq!(ftx.tag(), "FTX");
        assert_eq!(ftx.segment().element(4), ["a'b?", "c+d"]);
        assert_eq!(walk.next().expect("three").expect("sound").tag(), "UNZ");
        assert!(walk.next().is_none());
        // A released character in a tag is read as a segment is cut.
        let released = Interchange::walk("U?NB+1'").expect("default").next();
        assert_eq!(released.expect("one").expect("sound").tag(), "UNB");
        // The tag's element ends the tag; what else it holds is not read.
        let composite = Interchange::parse("UNH:1+X'").expect("parses");
        assert_eq!(composite.segments()[0].tag, "UNH");
        assert_eq!(composite.segments()[0].element(1), ["X"]);
    }

    #[test]
    fn a_walk_refuses_what_it_reaches_and_ends_there() {
        let refused = |text: &str| {
            Interchange::walk(text)
                .and_then(Iterator::collect::<Result<Vec<_>, _>>)
                .expect_err("malformed")
                .message
        };
        assert_eq!(
            refused("UNB+1'UNZ+1?"),
            "the text ends on a release character"
        );
        assert_eq!(refused("UNB+1'UNZ+1?'"), "a segment without its terminator");
        assert_eq!(refused("UNB+1'unz+1'"), "\"unz\" is not a segment tag");
        assert_eq!(refused("UNB+1''"), "\"\" is not a segment tag");
        // Nothing stands before an interchange's first segment.
        assert_eq!(refused(" UNB+1'UNZ+0+1'"), "\" UNB\" is not a segment tag");
        assert_eq!(
            refused("UNA:+.? '\r\nUNB+1'"),
            "\"\\r\\nUNB\" is not a segment tag"
        );
        assert_eq!(refused(" \n"), "no segment at all");
        assert_eq!(refused(""), "no segment at all");
        assert_eq!(
            refused("UNA:+.? 'UNB+1?"),
            "the text ends on a release character"
        );
        // The UNA is refused in the shape's words, as the shape reads it.
        assert_eq!(refused("UNA:+."), "a UNA shorter than its nine characters");
        assert_eq!(
            refused("UNA:+.? \u{e9}UNB+1\u{e9}"),
            "a UNA with a service character outside ASCII"
        );
        // What lies past the segments taken is not read.
        let mut walk = Interchange::walk("UNB+1'UNH+1'unz+1").expect("default");
        assert!(walk.next().expect("one").is_ok());
        assert!(walk.next().expect("two").is_ok());
        assert!(walk.next().expect("three").is_err());
        assert!(walk.next().is_none(), "a walk ends where it refuses");
        let mut walk = Interchange::walk("UNB+1'unz+1'UNZ+1'").expect("default");
        assert!(walk.next().expect("one").is_ok());
        assert!(walk.next().expect("two").is_err());
        assert!(walk.next().is_none(), "a walk ends where a tag is refused");
    }
}
