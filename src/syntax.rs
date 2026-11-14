//! ISO 9735 syntax: an interchange read into segments, written back in its
//! own service characters, and the soundness checks the service segments
//! carry. The segments are cut by the one walk, [`Walk`].
//!
//! An issue's `code` is `malformed` when the text cannot be read as segments at
//! all, else `envelope` for a service-segment departure; its `path` is
//! `segment N (TAG)`.

use contract::ValidationIssue;
// The segment is the capability's: EDIFACT and X12 read the same shape
// (ADR-0044); the walk that cuts it out of an interchange is this crate's.
use contract::segment::Segment;

use crate::walk::Walk;

/// The service characters in force.
#[derive(Clone, Copy, Debug)]
pub struct ServiceCharacters {
    pub component: char,
    pub data: char,
    pub decimal: char,
    pub release: char,
    pub terminator: char,
}

impl Default for ServiceCharacters {
    fn default() -> Self {
        Self {
            component: ':',
            data: '+',
            decimal: '.',
            release: '?',
            terminator: '\'',
        }
    }
}

/// A read interchange.
pub struct Interchange {
    /// The `UNA` advice as it was written, where there was one.
    advice: Option<String>,
    characters: ServiceCharacters,
    segments: Vec<Segment>,
}

impl Interchange {
    /// Read `text` with its own service characters.
    ///
    /// # Errors
    /// The one `malformed` issue when the text is not segments at all.
    pub fn parse(text: &str) -> Result<Self, ValidationIssue> {
        let walk = Walk::over(text)?;
        let advice = walk.advice().map(str::to_string);
        let characters = walk.characters();
        let segments = walk
            .map(|lying| lying.map(|lying| lying.segment()))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            advice,
            characters,
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

    /// The service characters in force.
    #[must_use]
    pub const fn characters(&self) -> ServiceCharacters {
        self.characters
    }

    /// The interchange written with its own service characters, its `UNA`
    /// as it was, every service character in a value released.
    #[must_use]
    pub fn written(&self) -> String {
        let c = self.characters;
        let mut text = self.advice.clone().unwrap_or_default();
        for segment in &self.segments {
            text.push_str(&segment.tag);
            for element in &segment.elements {
                text.push(c.data);
                for (index, component) in element.iter().enumerate() {
                    if index > 0 {
                        text.push(c.component);
                    }
                    for ch in component.chars() {
                        if [c.component, c.data, c.release, c.terminator].contains(&ch) {
                            text.push(c.release);
                        }
                        text.push(ch);
                    }
                }
            }
            text.push(c.terminator);
        }
        text
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
        assert_eq!(read.written(), text);
        read.segments_mut()[1].elements[3][0] = "x:y".to_string();
        assert_eq!(
            read.written(),
            "UNA:+.? 'UNB+UNOC:3+S+R'FTX+AAI+++x?:y'UNZ+0+R'"
        );
        let bare = Interchange::parse("UNB+UNOC:3+S+R'UNZ+0+R'").expect("parses");
        assert_eq!(bare.written(), "UNB+UNOC:3+S+R'UNZ+0+R'");
    }

    #[test]
    fn una_sets_the_service_characters_and_release_escapes() {
        let text =
            "UNA|^,! ~UNB^UNOC|3^S^R^260907|1345^REF~FTX^AAI^^^Price is 5!^ per unit~UNZ^0^REF~";
        let read = Interchange::parse(text).expect("parses");
        assert_eq!(read.segments()[1].simple(4), "Price is 5^ per unit");
        assert_eq!(read.segments()[0].element(1), ["UNOC", "3"]);
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
}
