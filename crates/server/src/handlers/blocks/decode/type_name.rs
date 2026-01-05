//! Type name extraction visitor.
//!
//! This module provides `GetTypeName`, a visitor which retrieves the name of a type
//! from its path. Based on the example from subxt_historic/examples/extrinsics.rs.

use scale_decode::{
    Visitor,
    visitor::{
        TypeIdFor, Unexpected,
        types::{Composite, Sequence, Variant},
    },
};
use scale_info_legacy::LookupName;
use scale_type_resolver::TypeResolver;

/// A visitor which obtains type names from types.
pub struct GetTypeName<R> {
    marker: core::marker::PhantomData<R>,
}

impl<R> GetTypeName<R> {
    /// Construct our TypeName visitor.
    pub fn new() -> Self {
        GetTypeName {
            marker: core::marker::PhantomData,
        }
    }
}

impl<R> Default for GetTypeName<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> Visitor for GetTypeName<R>
where
    R: TypeResolver,
    R::TypeId: TryInto<LookupName>,
{
    type Value<'scale, 'resolver> = Option<&'resolver str>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

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
