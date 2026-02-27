// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visitor pattern implementation for extracting relay_parent_number from ParachainInherentData.
//!
//! This approach uses scale_decode's Visitor trait to traverse the SCALE-encoded structure
//! and extract only the relay_parent_number field from PersistedValidationData, without
//! needing exact type definitions that match the runtime's type registry.
//!
//! The structure we're navigating:
//! ```text
//! ParachainInherentData {
//!     validation_data: PersistedValidationData {
//!         parent_head: HeadData (Vec<u8>),
//!         relay_parent_number: u32,  // <-- This is what we need
//!         relay_parent_storage_root: H256,
//!         max_pov_size: u32,
//!     },
//!     relay_chain_state: StorageProof,
//!     downward_messages: Vec<InboundDownwardMessage>,
//!     horizontal_messages: BTreeMap<ParaId, Vec<InboundHrmpMessage>>,
//!     // ... possibly more fields in newer runtimes
//! }
//! ```

use scale_decode::{
    Visitor,
    visitor::TypeIdFor,
    visitor::types::{Array, BitSequence, Composite, Sequence, Str, Tuple, Variant},
};
use scale_type_resolver::TypeResolver;

/// Result of extracting relay_parent_number. None if not found yet.
#[derive(Debug, Clone)]
pub enum ExtractedValue {
    /// We found the relay_parent_number
    Found(u32),
    /// We haven't found it yet (or the field doesn't contain it)
    NotFound,
}


/// Error type for visitor operations
#[derive(Debug, thiserror::Error)]
pub enum VisitorError {
    #[error("Decode error: {0}")]
    Decode(#[from] scale_decode::visitor::DecodeError),
    #[error("Cannot decode bit sequence: {0}")]
    BitSequence(#[from] parity_scale_codec::Error),
}

/// Visitor that extracts relay_parent_number from the first field (validation_data)
/// of ParachainInherentData.
pub struct RelayParentExtractor<'r, R> {
    resolver: &'r R,
    /// Tracks the nesting depth and which field we're looking at
    path: Vec<FieldContext>,
}

#[derive(Debug, Clone)]
enum FieldContext {
    /// First field of ParachainInherentData (should be validation_data)
    ValidationData,
    /// Second field of PersistedValidationData (should be relay_parent_number)
    RelayParentNumber,
}

impl<'r, R> RelayParentExtractor<'r, R> {
    pub fn new(resolver: &'r R) -> Self {
        Self {
            resolver,
            path: Vec::new(),
        }
    }

    fn with_context(&self, ctx: FieldContext) -> Self {
        let mut new_path = self.path.clone();
        new_path.push(ctx);
        Self {
            resolver: self.resolver,
            path: new_path,
        }
    }

    fn is_looking_for_relay_parent(&self) -> bool {
        matches!(
            self.path.as_slice(),
            [FieldContext::ValidationData, FieldContext::RelayParentNumber]
        )
    }
}

impl<'r, R: TypeResolver> Visitor for RelayParentExtractor<'r, R> {
    type Value<'scale, 'resolver> = ExtractedValue;
    type Error = VisitorError;
    type TypeResolver = R;

    // We're looking for a u32, so implement visit_u32 to capture relay_parent_number
    fn visit_u32<'scale, 'resolver>(
        self,
        value: u32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        if self.is_looking_for_relay_parent() {
            Ok(ExtractedValue::Found(value))
        } else {
            Ok(ExtractedValue::NotFound)
        }
    }

    // Handle composite types (structs) - this is where we navigate the nested structure
    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // At the top level (empty path), we want the first field (validation_data)
        if self.path.is_empty() {
            // Get the first field which should be validation_data
            if let Some(field) = value.next() {
                let field = field?;
                let nested_visitor = self.with_context(FieldContext::ValidationData);
                return field.decode_with_visitor(nested_visitor);
            }
        }
        // Inside validation_data, we want the second field (relay_parent_number)
        else if matches!(self.path.as_slice(), [FieldContext::ValidationData]) {
            // Skip first field (parent_head)
            if let Some(field) = value.next() {
                let _ = field?; // Skip parent_head
            }
            // Get second field (relay_parent_number)
            if let Some(field) = value.next() {
                let field = field?;
                let nested_visitor = self.with_context(FieldContext::RelayParentNumber);
                return field.decode_with_visitor(nested_visitor);
            }
        }

        Ok(ExtractedValue::NotFound)
    }

    // Default implementations for types we don't care about - just return NotFound

    fn visit_bool<'scale, 'resolver>(
        self,
        _value: bool,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_char<'scale, 'resolver>(
        self,
        _value: char,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_u8<'scale, 'resolver>(
        self,
        _value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_u16<'scale, 'resolver>(
        self,
        _value: u16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_u64<'scale, 'resolver>(
        self,
        _value: u64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_u128<'scale, 'resolver>(
        self,
        _value: u128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_u256<'resolver>(
        self,
        _value: &[u8; 32],
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'_, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_i8<'scale, 'resolver>(
        self,
        _value: i8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_i16<'scale, 'resolver>(
        self,
        _value: i16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_i32<'scale, 'resolver>(
        self,
        _value: i32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_i64<'scale, 'resolver>(
        self,
        _value: i64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_i128<'scale, 'resolver>(
        self,
        _value: i128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_i256<'resolver>(
        self,
        _value: &[u8; 32],
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'_, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_sequence<'scale, 'resolver>(
        self,
        _value: &mut Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_array<'scale, 'resolver>(
        self,
        _value: &mut Array<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_tuple<'scale, 'resolver>(
        self,
        _value: &mut Tuple<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_str<'scale, 'resolver>(
        self,
        _value: &mut Str<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        _value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }

    fn visit_bitsequence<'scale, 'resolver>(
        self,
        _value: &mut BitSequence<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(ExtractedValue::NotFound)
    }
}

