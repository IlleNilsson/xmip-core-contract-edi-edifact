//! ISO 9735 syntax: reading an interchange into segments, and the soundness
//! checks the service segments carry.
//!
//! An issue's `code` is `malformed` when the text cannot be read as segments at
//! all, else `envelope` for a service-segment departure; its `path` is
//! `segment N (TAG)`.

use contract::ValidationIssue;
// The segment is the capability's: EDIFACT and X12 read the same shape
// (ADR-0044); the syntax that cuts it out of an interchange is this file's.
pub use contract::segment::Segment;

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
    segments: Vec<Segment>,
}

impl Interchange {
    /// Read `text` with its own service characters.
    ///
    /// # Errors
    /// The one `malformed` issue when the text is not segments at all.
    pub fn parse(text: &str) -> Result<Self, ValidationIssue> {
        let (characters, body) = match text.strip_prefix("UNA") {
            Some(rest) if rest.chars().count() >= 6 => {
                let mut c = rest.chars();
                let characters = ServiceCharacters {
                    component: c.next().unwrap_or(':'),
                    data: c.next().unwrap_or('+'),
                    decimal: c.next().unwrap_or('.'),
                    release: c.next().unwrap_or('?'),
                    terminator: c.nth(1).unwrap_or('\''),
                };
                (characters, c.as_str())
            }
            Some(_) => {
                return Err(ValidationIssue::malformed(
                    "UNA is shorter than its six service characters",
                ));
            }
            None => (ServiceCharacters::default(), text),
        };
        let segments = segments(body, characters)?;
        if segments.is_empty() {
            return Err(ValidationIssue::malformed("no segment"));
        }
        Ok(Self { segments })
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
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
        let envelope = |message: &str, n: usize| ValidationIssue::at("envelope", message, &at(n));
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

fn segments(body: &str, c: ServiceCharacters) -> Result<Vec<Segment>, ValidationIssue> {
    let mut segments = Vec::new();
    let mut elements: Vec<Vec<String>> = vec![vec![String::new()]];
    let mut chars = body.chars().peekable();
    let mut released = false;
    while let Some(ch) = chars.next() {
        if released {
            push(&mut elements, ch);
            released = false;
        } else if ch == c.release {
            released = true;
        } else if ch == c.terminator {
            segments.push(segment(std::mem::take(&mut elements))?);
            elements = vec![vec![String::new()]];
            while chars.peek().is_some_and(|n| n.is_whitespace()) {
                chars.next();
            }
        } else if ch == c.data {
            elements.push(vec![String::new()]);
        } else if ch == c.component {
            if let Some(last) = elements.last_mut() {
                last.push(String::new());
            }
        } else {
            push(&mut elements, ch);
        }
    }
    if released {
        return Err(ValidationIssue::malformed(
            "the text ends on a release character",
        ));
    }
    if elements.len() > 1 || !elements[0][0].is_empty() {
        return Err(ValidationIssue::malformed(
            "the last segment has no terminator",
        ));
    }
    Ok(segments)
}

fn push(elements: &mut [Vec<String>], ch: char) {
    if let Some(component) = elements.last_mut().and_then(|e| e.last_mut()) {
        component.push(ch);
    }
}

fn segment(mut elements: Vec<Vec<String>>) -> Result<Segment, ValidationIssue> {
    let tag = elements.remove(0).into_iter().next().unwrap_or_default();
    let sound = tag.len() == 3
        && tag
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if !sound {
        return Err(ValidationIssue::malformed(&format!(
            "{tag:?} is not a segment tag"
        )));
    }
    Ok(Segment { tag, elements })
}

#[cfg(test)]
mod tests {
    use super::*;

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
