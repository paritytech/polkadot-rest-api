//! Documentation extraction from runtime metadata.
//!
//! This module provides zero-copy access to documentation strings for events and calls
//! from subxt metadata.

use serde::Serialize;

/// Zero-copy reference to documentation strings from metadata.
pub struct Docs<'a> {
    docs: &'a [String],
}

impl<'a> Docs<'a> {
    /// Create docs from a slice of Strings
    fn from_strings(docs: &'a [String]) -> Option<Self> {
        if docs.is_empty() || docs.iter().all(|s| s.is_empty()) {
            None
        } else {
            Some(Self { docs })
        }
    }

    /// Get event documentation from subxt Metadata.
    pub fn for_event_subxt(
        metadata: &'a subxt::Metadata,
        pallet_name: &str,
        event_name: &str,
    ) -> Option<Docs<'a>> {
        let pallet = metadata.pallet_by_name(pallet_name)?;
        let variants = pallet.event_variants()?;
        for variant in variants {
            if variant.name.eq_ignore_ascii_case(event_name) {
                return Docs::from_strings(&variant.docs);
            }
        }
        None
    }

    /// Get call documentation from subxt Metadata.
    pub fn for_call_subxt(
        metadata: &'a subxt::Metadata,
        pallet_name: &str,
        call_name: &str,
    ) -> Option<Docs<'a>> {
        let pallet = metadata.pallet_by_name(pallet_name)?;
        let variants = pallet.call_variants()?;
        for variant in variants {
            if variant.name.eq_ignore_ascii_case(call_name) {
                return Docs::from_strings(&variant.docs);
            }
        }
        None
    }
}

impl std::fmt::Display for Docs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for doc in self.docs {
            if !first {
                writeln!(f)?;
            }
            write!(f, "{}", doc)?;
            first = false;
        }
        Ok(())
    }
}

impl Serialize for Docs<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
