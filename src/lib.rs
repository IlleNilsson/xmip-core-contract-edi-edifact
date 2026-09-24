#![forbid(unsafe_code)]

//! The UN/EDIFACT content contract — a technology of `xmip-core-contract`.
//!
//! Two claims, decided 2026-09-07: **well-formedness is a given** and
//! **conformance is a given once a contract is named**.
//!
//! Well-formed here is a *sound interchange*, ISO 9735 syntax: segments read
//! with the service characters in force (the defaults, or the `UNA` the
//! interchange opens with, release character honoured); `UNB` opens and `UNZ`
//! closes with the same reference and the right message count; every `UNH` is
//! closed by a `UNT` with the same reference and the right segment count;
//! `UNG`/`UNE` groups likewise. That is what a partner's interchange must
//! satisfy before any message in it means anything.
//!
//! Conformance is the *message type*: a Location that names this contract with
//! `ORDERS:D:96A` bound has every message's `UNH` S009 held to that type,
//! version and release, and `ORDERS` alone holds the type. Every EDIFACT
//! directory is a version of this one technology and lives in this repository
//! (owner, 2026-09-07); the segment tables that would hold a message to its
//! directory's structure are the next layer here, not another repository.

pub mod syntax;

use contract::{
    Contract, ContractDescriptor, ContractError, ContractFactory, ContractId, ValidationIssue,
    ValidationResult,
};
use stream::Stream;
use syntax::{Interchange, Segment};

/// The bound message type: type, and optionally version and release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageType {
    pub name: String,
    pub version: Option<String>,
    pub release: Option<String>,
}

impl MessageType {
    /// `ORDERS`, `ORDERS:D:96A`, `INVOIC:D:01B`.
    ///
    /// # Errors
    /// An empty type, or more than three parts.
    pub fn parse(reference: &str) -> Result<Self, ContractError> {
        let parts: Vec<&str> = reference.split(':').map(str::trim).collect();
        match parts.as_slice() {
            [name] if !name.is_empty() => Ok(Self {
                name: name.to_ascii_uppercase(),
                version: None,
                release: None,
            }),
            [name, version, release] if !name.is_empty() => Ok(Self {
                name: name.to_ascii_uppercase(),
                version: Some(version.to_ascii_uppercase()),
                release: Some(release.to_ascii_uppercase()),
            }),
            _ => Err(ContractError {
                message: format!("{reference:?} is not TYPE or TYPE:VERSION:RELEASE"),
            }),
        }
    }

    fn reference(&self) -> String {
        match (&self.version, &self.release) {
            (Some(version), Some(release)) => format!("{}:{version}:{release}", self.name),
            _ => self.name.clone(),
        }
    }
}

/// The EDIFACT contract, bare or bound to a message type.
pub struct Edifact {
    descriptor: ContractDescriptor,
    message_type: Option<MessageType>,
}

impl Edifact {
    /// A sound interchange, of any message types.
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptor: descriptor("edi-edifact"),
            message_type: None,
        }
    }

    /// A sound interchange whose every message is `message_type`.
    #[must_use]
    pub fn of(message_type: MessageType) -> Self {
        Self {
            descriptor: descriptor(&format!("edi-edifact:{}", message_type.reference())),
            message_type: Some(message_type),
        }
    }

    /// Whether a message type is bound.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.message_type.is_some()
    }
}

impl Default for Edifact {
    fn default() -> Self {
        Self::new()
    }
}

fn descriptor(id: &str) -> ContractDescriptor {
    ContractDescriptor {
        id: ContractId(id.to_string()),
        version: "1".to_string(),
        representation: "application/EDIFACT".to_string(),
    }
}

impl Contract for Edifact {
    fn descriptor(&self) -> &ContractDescriptor {
        &self.descriptor
    }

    fn identify(&self, stream: &Stream) -> Result<bool, ContractError> {
        if stream.media_type().is_some_and(|m| {
            m.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/EDIFACT")
        }) {
            return Ok(true);
        }
        let head = &stream.bytes()[..stream.bytes().len().min(3)];
        Ok(head == b"UNA" || head == b"UNB")
    }

    fn validate(&self, stream: &Stream) -> Result<ValidationResult, ContractError> {
        let text = match std::str::from_utf8(stream.bytes()) {
            Ok(text) => text,
            Err(error) => {
                return Ok(ValidationResult::of(vec![ValidationIssue::malformed(
                    &format!("not text: {error}"),
                )]));
            }
        };
        let interchange = match Interchange::parse(text) {
            Ok(interchange) => interchange,
            Err(unsound) => return Ok(ValidationResult::of(vec![unsound])),
        };
        let mut issues = interchange.soundness();
        if let Some(wanted) = &self.message_type {
            for (ordinal, header) in interchange.message_headers() {
                if let Some(message) = mismatch(wanted, header) {
                    issues.push(ValidationIssue::new(
                        "message-type",
                        &message,
                        Some(format!("message {ordinal}")),
                    ));
                }
            }
        }
        Ok(ValidationResult::of(issues))
    }
}

/// Why a `UNH` is not the bound type, or `None` when it is.
fn mismatch(wanted: &MessageType, header: &Segment) -> Option<String> {
    let s009 = header.element(2);
    let actual = |index: usize| {
        s009.get(index)
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or_default()
    };
    if actual(0) != wanted.name {
        return Some(format!("is {}, the contract is {}", actual(0), wanted.name));
    }
    if let (Some(version), Some(release)) = (&wanted.version, &wanted.release)
        && (actual(1) != *version || actual(2) != *release)
    {
        return Some(format!(
            "is {} {}:{}, the contract is {}",
            wanted.name,
            actual(1),
            actual(2),
            wanted.reference()
        ));
    }
    None
}

/// Loads the contract a Location names: an empty reference is the bare
/// contract, anything else is a message type, `ORDERS` or `ORDERS:D:96A`.
pub struct EdifactFactory;

impl ContractFactory for EdifactFactory {
    fn technology(&self) -> &'static str {
        "edi-edifact"
    }

    fn load(&self, reference: &str) -> Result<Box<dyn Contract>, ContractError> {
        if reference.trim().is_empty() {
            return Ok(Box::new(Edifact::new()));
        }
        Ok(Box::new(Edifact::of(MessageType::parse(reference)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contract::fixture::stream;
    use xcore::StreamId;

    const ORDERS: &str = "UNA:+.? 'UNB+UNOC:3+SENDER+RECEIVER+260907:1345+REF001'\
UNH+1+ORDERS:D:96A:UN'BGM+220+PO4711'DTM+137:20260907:102'NAD+BY+ACME'\
LIN+1++X001:SA'QTY+21:2'UNS+S'CNT+2:1'UNT+9+1'UNZ+1+REF001'";

    #[test]
    fn a_sound_interchange_holds_bare() {
        let held = Edifact::new().validate(&stream(ORDERS)).expect("validates");
        assert!(held.valid, "issues: {:?}", held.issues);
    }

    #[test]
    fn a_bound_type_holds_and_names_the_message_that_departs() {
        let bound = Edifact::of(MessageType::parse("ORDERS:D:96A").expect("type"));
        assert_eq!(bound.descriptor().id.0, "edi-edifact:ORDERS:D:96A");
        assert!(bound.validate(&stream(ORDERS)).expect("validates").valid);
        let invoic = Edifact::of(MessageType::parse("INVOIC").expect("type"));
        let held = invoic.validate(&stream(ORDERS)).expect("validates");
        assert_eq!(held.issues[0].code, "message-type");
        assert_eq!(held.issues[0].path.as_deref(), Some("message 1"));
        let d01b = Edifact::of(MessageType::parse("ORDERS:D:01B").expect("type"));
        let held = d01b.validate(&stream(ORDERS)).expect("validates");
        assert!(
            held.issues[0].message.contains("D:96A"),
            "{}",
            held.issues[0].message
        );
    }

    #[test]
    fn identifies_by_media_type_or_service_segment() {
        assert!(
            Edifact::new()
                .identify(&stream(ORDERS))
                .expect("identifies")
        );
        assert!(
            !Edifact::new()
                .identify(&stream("<xml/>"))
                .expect("identifies")
        );
        let typed = Stream::new(
            StreamId::new(1),
            b"x".to_vec(),
            Some("application/EDIFACT".into()),
        );
        assert!(Edifact::new().identify(&typed).expect("identifies"));
    }

    #[test]
    fn the_factory_reads_the_type_off_the_reference() {
        let factory = EdifactFactory;
        assert_eq!(factory.technology(), "edi-edifact");
        assert_eq!(
            factory.load("").expect("bare").descriptor().id.0,
            "edi-edifact"
        );
        assert_eq!(
            factory
                .load("orders:d:96a")
                .expect("typed")
                .descriptor()
                .id
                .0,
            "edi-edifact:ORDERS:D:96A"
        );
        assert!(factory.load("ORDERS:D").is_err());
    }
}
