//! Documentation extraction from runtime metadata.
//!
//! This module provides zero-copy access to documentation strings for events and calls
//! from runtime metadata. It supports all metadata versions V9-V16.

use serde::Serialize;

// ================================================================================================
// Docs Struct
// ================================================================================================

/// Zero-copy reference to documentation strings from metadata.
/// Supports all metadata versions V9-V16 without expensive encode/decode operations.
pub struct Docs<'a> {
    inner: DocsInner<'a>,
}

/// Internal representation of docs that can hold different reference types
/// depending on the metadata version.
enum DocsInner<'a> {
    /// Reference to Vec<String> (V14+ metadata uses this format)
    Strings(&'a [String]),
    /// Reference to static str slice (V9-V13 compile-time metadata)
    Static(&'a [&'static str]),
}

impl<'a> Docs<'a> {
    /// Create docs from a slice of Strings (V14+ metadata)
    fn from_strings(docs: &'a [String]) -> Option<Self> {
        if docs.is_empty() || docs.iter().all(|s| s.is_empty()) {
            None
        } else {
            Some(Self {
                inner: DocsInner::Strings(docs),
            })
        }
    }

    /// Create docs from a static str slice (V9-V13 metadata)
    fn from_static(docs: &'a [&'static str]) -> Option<Self> {
        if docs.is_empty() || docs.iter().all(|s| s.is_empty()) {
            None
        } else {
            Some(Self {
                inner: DocsInner::Static(docs),
            })
        }
    }

    /// Get event documentation from RuntimeMetadata.
    /// Works with all metadata versions V9-V16.
    pub fn for_event(
        metadata: &'a frame_metadata::RuntimeMetadata,
        pallet_name: &str,
        event_name: &str,
    ) -> Option<Docs<'a>> {
        get_event_docs(metadata, pallet_name, event_name)
    }

    /// Get call documentation from RuntimeMetadata.
    /// Works with all metadata versions V9-V16.
    pub fn for_call(
        metadata: &'a frame_metadata::RuntimeMetadata,
        pallet_name: &str,
        call_name: &str,
    ) -> Option<Docs<'a>> {
        get_call_docs(metadata, pallet_name, call_name)
    }
}

impl std::fmt::Display for Docs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            DocsInner::Strings(docs) => {
                let mut first = true;
                for doc in *docs {
                    if !first {
                        writeln!(f)?;
                    }
                    write!(f, "{}", doc)?;
                    first = false;
                }
                Ok(())
            }
            DocsInner::Static(docs) => {
                let mut first = true;
                for doc in *docs {
                    if !first {
                        writeln!(f)?;
                    }
                    write!(f, "{}", doc)?;
                    first = false;
                }
                Ok(())
            }
        }
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

// ================================================================================================
// Documentation Lookup Functions
// ================================================================================================

/// Extract event documentation from metadata (V9-V16)
/// Returns a zero-copy Docs reference when possible.
fn get_event_docs<'a>(
    metadata: &'a frame_metadata::RuntimeMetadata,
    pallet_name: &str,
    event_name: &str,
) -> Option<Docs<'a>> {
    use frame_metadata::RuntimeMetadata::*;
    use frame_metadata::decode_different::DecodeDifferent;

    // Helper to extract string from DecodeDifferent
    fn extract_str<'a>(s: &'a DecodeDifferent<&'static str, String>) -> &'a str {
        match s {
            DecodeDifferent::Decoded(v) => v.as_str(),
            DecodeDifferent::Encode(s) => s,
        }
    }

    // Helper to create Docs from DecodeDifferent docs
    fn docs_from_decode_different<'a>(
        docs: &'a DecodeDifferent<&'static [&'static str], Vec<String>>,
    ) -> Option<Docs<'a>> {
        match docs {
            DecodeDifferent::Decoded(v) => Docs::from_strings(v),
            DecodeDifferent::Encode(s) => Docs::from_static(s),
        }
    }

    match metadata {
        V9(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V10(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V11(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V12(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V13(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V14(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(event_ty) = &pallet.event
                    && let Some(ty) = meta.types.resolve(event_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(event_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V15(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(event_ty) = &pallet.event
                    && let Some(ty) = meta.types.resolve(event_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(event_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V16(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(event_ty) = &pallet.event
                    && let Some(ty) = meta.types.resolve(event_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(event_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract call documentation from metadata (V9-V16)
/// Returns a zero-copy Docs reference when possible.
fn get_call_docs<'a>(
    metadata: &'a frame_metadata::RuntimeMetadata,
    pallet_name: &str,
    call_name: &str,
) -> Option<Docs<'a>> {
    use frame_metadata::RuntimeMetadata::*;
    use frame_metadata::decode_different::DecodeDifferent;

    // Helper to extract string from DecodeDifferent
    fn extract_str<'a>(s: &'a DecodeDifferent<&'static str, String>) -> &'a str {
        match s {
            DecodeDifferent::Decoded(v) => v.as_str(),
            DecodeDifferent::Encode(s) => s,
        }
    }

    // Helper to create Docs from DecodeDifferent docs
    fn docs_from_decode_different<'a>(
        docs: &'a DecodeDifferent<&'static [&'static str], Vec<String>>,
    ) -> Option<Docs<'a>> {
        match docs {
            DecodeDifferent::Decoded(v) => Docs::from_strings(v),
            DecodeDifferent::Encode(s) => Docs::from_static(s),
        }
    }

    match metadata {
        V9(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V10(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V11(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V12(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V13(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V14(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(call_ty) = &pallet.calls
                    && let Some(ty) = meta.types.resolve(call_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(call_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V15(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(call_ty) = &pallet.calls
                    && let Some(ty) = meta.types.resolve(call_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(call_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V16(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(call_ty) = &pallet.calls
                    && let Some(ty) = meta.types.resolve(call_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(call_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}
