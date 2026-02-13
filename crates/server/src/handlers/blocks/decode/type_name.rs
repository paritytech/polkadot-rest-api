// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Type name extraction visitor.
//!
//! This module provides `GetTypeName`, a visitor which retrieves the name of a type
//! from its path. Updated for subxt 0.50.0 which uses PortableRegistry.

use scale_decode::{
    Visitor,
    visitor::{
        TypeIdFor, Unexpected,
        types::{Composite, Sequence, Variant},
    },
};
use scale_info::PortableRegistry;

/// A visitor which obtains type names from types.
/// This version is specialized for PortableRegistry (u32 type IDs).
pub struct GetTypeName;

impl GetTypeName {
    /// Construct our TypeName visitor.
    pub fn new() -> Self {
        GetTypeName
    }
}

impl Default for GetTypeName {
    fn default() -> Self {
        Self::new()
    }
}

impl Visitor for GetTypeName {
    type Value<'scale, 'resolver> = Option<&'resolver str>;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    // Look at the path of types that have paths and return the ident from that.
    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(value.path().last())
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(value.path().last())
    }

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(value.path().last())
    }

    // Else, we return nothing as we can't find a name for the type.
    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(None)
    }
}
