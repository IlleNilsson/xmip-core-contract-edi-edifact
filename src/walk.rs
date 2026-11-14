//! An interchange walked a segment at a time in its own service
//! characters. What reads one segment cuts only that one and walks the rest
//! uncut, and [`Interchange::parse`](crate::syntax::Interchange::parse) cuts
//! every one into the tree a write edits: one walk, so a segment is cut, a
//! release honored and a malformed interchange refused in the same words by
//! both.
//!
//! A segment is the text from its tag to its terminator, the first one not
//! released; the whitespace after a terminator is skipped. It is cut into
//! elements and components only when asked.

use std::borrow::Cow;

use contract::ValidationIssue;
// The segment is the capability's: EDIFACT and X12 read the same shape
// (ADR-0044); the syntax that cuts it out of an interchange is this crate's.
use contract::segment::Segment;

use crate::syntax::ServiceCharacters;

/// The segments of an interchange, one at a time, in the order they lie.
#[derive(Clone, Debug)]
pub struct Walk<'a> {
    advice: Option<&'a str>,
    characters: ServiceCharacters,
    /// The text not walked yet; none once the walk has ended.
    rest: Option<&'a str>,
    walked: bool,
}

impl<'a> Walk<'a> {
    /// `text`, read with the service characters its `UNA` advises, else the
    /// default set.
    ///
    /// # Errors
    /// The `malformed` issue a `UNA` shorter than its six service characters
    /// is.
    pub fn over(text: &'a str) -> Result<Self, ValidationIssue> {
        let (characters, body) = match text.strip_prefix("UNA") {
            Some(advice) => advised(advice)?,
            None => (ServiceCharacters::default(), text),
        };
        let head = &text[..text.len() - body.len()];
        Ok(Self {
            advice: Some(head).filter(|head| !head.is_empty()),
            characters,
            rest: Some(body),
            walked: false,
        })
    }

    /// The `UNA` advice as it was written, where there was one.
    #[must_use]
    pub const fn advice(&self) -> Option<&'a str> {
        self.advice
    }

    /// The service characters in force.
    #[must_use]
    pub const fn characters(&self) -> ServiceCharacters {
        self.characters
    }

    /// What `rest`, the text after the last terminator, says as the walk
    /// ends: nothing when it holds nothing a segment would, else why the
    /// interchange is malformed.
    fn end(&self, rest: &str) -> Option<Result<Lying<'a>, ValidationIssue>> {
        let (elements, released) = elements(rest, self.characters);
        let refused = if released {
            "the text ends on a release character"
        } else if elements.len() > 1 || elements.first().is_some_and(|e| !e[0].is_empty()) {
            "the last segment has no terminator"
        } else if !self.walked {
            "no segment"
        } else {
            return None;
        };
        Some(Err(ValidationIssue::malformed(refused)))
    }
}

impl<'a> Iterator for Walk<'a> {
    type Item = Result<Lying<'a>, ValidationIssue>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.rest.take()?;
        let c = self.characters;
        let Some(at) = terminator(rest, c) else {
            return self.end(rest);
        };
        let lying = Lying::cut(&rest[..at], c);
        if lying.is_ok() {
            self.walked = true;
            self.rest = Some(rest[at + c.terminator.len_utf8()..].trim_start());
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
    characters: ServiceCharacters,
}

impl<'a> Lying<'a> {
    /// `text`, one segment without its terminator, its tag read and held to
    /// three capital letters or digits.
    fn cut(text: &'a str, c: ServiceCharacters) -> Result<Self, ValidationIssue> {
        let tag = match text.find([c.release, c.data, c.component]) {
            // A released character in the tag is read as a segment is cut.
            Some(at) if text[at..].starts_with(c.release) => {
                let (elements, _) = elements(text, c);
                Cow::Owned(elements.into_iter().flatten().next().unwrap_or_default())
            }
            Some(at) => Cow::Borrowed(&text[..at]),
            None => Cow::Borrowed(text),
        };
        let sound = tag.len() == 3
            && tag
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit());
        if !sound {
            return Err(ValidationIssue::malformed(format!(
                "{tag:?} is not a segment tag"
            )));
        }
        Ok(Self {
            tag,
            text,
            characters: c,
        })
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
        let (mut elements, _) = elements(self.text, self.characters);
        // The tag is the first element's first component, as it was read.
        let tag = elements.remove(0).into_iter().next().unwrap_or_default();
        Segment { tag, elements }
    }
}

/// The service characters a `UNA`'s six advise, and the text after them.
fn advised(advice: &str) -> Result<(ServiceCharacters, &str), ValidationIssue> {
    let mut read = advice.chars();
    let mut next = || {
        read.next().ok_or_else(|| {
            ValidationIssue::malformed("UNA is shorter than its six service characters")
        })
    };
    let characters = ServiceCharacters {
        component: next()?,
        data: next()?,
        decimal: next()?,
        release: next()?,
        terminator: {
            // The fifth is reserved.
            next()?;
            next()?
        },
    };
    Ok((characters, read.as_str()))
}

/// Where the first terminator in `text` that is not released lies, `text`
/// starting where a segment does. A terminator is released when an odd run
/// of release characters stands before it, each pair releasing the second:
/// so it is found by one search for the terminator alone, and the release
/// characters are counted only where one is met.
fn terminator(text: &str, c: ServiceCharacters) -> Option<usize> {
    // A release character that is the terminator too releases every one.
    if c.release == c.terminator {
        return None;
    }
    let mut from = 0;
    loop {
        let at = from + text[from..].find(c.terminator)?;
        let releases = text[..at]
            .chars()
            .rev()
            .take_while(|&ch| ch == c.release)
            .count();
        if releases % 2 == 0 {
            return Some(at);
        }
        from = at + c.terminator.len_utf8();
    }
}

/// `text`, one segment without its terminator, cut into elements and their
/// components, every release taken out; and whether it ends on a release.
/// A component no release stands in is copied whole from `text`; one that
/// holds a release is built a character at a time from it on.
fn elements(text: &str, c: ServiceCharacters) -> (Vec<Vec<String>>, bool) {
    let mut elements = Vec::new();
    let mut components = Vec::new();
    // Where the component being read starts, while no release stands in
    // it; and what it holds once one does.
    let mut start = Some(0);
    let mut built = String::new();
    let mut released = false;
    for (at, ch) in text.char_indices() {
        if released {
            built.push(ch);
            released = false;
        } else if ch == c.release {
            if let Some(from) = start.take() {
                built.push_str(&text[from..at]);
            }
            released = true;
        } else if ch == c.data || ch == c.component {
            components.push(match start {
                Some(from) => text[from..at].to_string(),
                None => std::mem::take(&mut built),
            });
            start = Some(at + ch.len_utf8());
            if ch == c.data {
                elements.push(std::mem::take(&mut components));
            }
        } else if start.is_none() {
            built.push(ch);
        }
    }
    components.push(match start {
        Some(from) => text[from..].to_string(),
        None => built,
    });
    elements.push(components);
    (elements, released)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_walk_yields_a_segment_at_a_time_and_cuts_one_only_when_asked() {
        let text = "UNA:+.? 'UNB+UNOC:3+S+R'\nFTX+AAI+++a?'b??:c?+d'UNZ+0+R'";
        let mut walk = Walk::over(text).expect("advised");
        assert_eq!(walk.advice(), Some("UNA:+.? '"));
        assert_eq!(walk.next().expect("one").expect("sound").tag(), "UNB");
        let ftx = walk.next().expect("two").expect("sound");
        assert_eq!(ftx.tag(), "FTX");
        assert_eq!(ftx.segment().element(4), ["a'b?", "c+d"]);
        assert_eq!(walk.next().expect("three").expect("sound").tag(), "UNZ");
        assert!(walk.next().is_none());
        // A released character in a tag is read as a segment is cut.
        let released = Walk::over("U?NB+1'").expect("default").next();
        assert_eq!(released.expect("one").expect("sound").tag(), "UNB");
    }

    #[test]
    fn a_walk_refuses_what_it_reaches_and_ends_there() {
        let refused = |text: &str| {
            Walk::over(text)
                .and_then(Iterator::collect::<Result<Vec<_>, _>>)
                .expect_err("malformed")
                .message
        };
        assert_eq!(
            refused("UNB+1'UNZ+1?"),
            "the text ends on a release character"
        );
        assert_eq!(
            refused("UNB+1'UNZ+1?'"),
            "the last segment has no terminator"
        );
        assert_eq!(refused("UNB+1'unz+1'"), "\"unz\" is not a segment tag");
        assert_eq!(refused(" \n"), "the last segment has no terminator");
        assert_eq!(refused(""), "no segment");
        assert_eq!(
            refused("UNA:+."),
            "UNA is shorter than its six service characters"
        );
        // What lies past the segments taken is not read.
        let mut walk = Walk::over("UNB+1'UNH+1'unz+1").expect("default");
        assert!(walk.next().expect("one").is_ok());
        assert!(walk.next().expect("two").is_ok());
        assert!(walk.next().expect("three").is_err());
        assert!(walk.next().is_none(), "a walk ends where it refuses");
    }
}
