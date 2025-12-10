use std::collections::HashMap;

/// Visitor for transforming SCALE-decoded values to JSON with proper enum serialization.
///
/// This visitor correctly handles the distinction between "basic" and "non-basic" enums
/// to match substrate-api-sidecar serialization:
///
/// - **Basic enums** (all variants have no data): serialize as strings
///   Example: `DispatchClass::Normal` -> `"Normal"`
///
/// - **Non-basic enums** (at least one variant has data): serialize as objects with lowercase keys
///   Example: `WeightLimit::Unlimited` -> `{"unlimited": null}`
///   Example: `WeightLimit::Limited(w)` -> `{"limited": <weight>}`
///   Example: `Completeness::Complete` -> `{"complete": {...}}`
///
/// The visitor uses the type resolver to inspect the full enum type definition
/// and determine whether any variant has associated data.
use scale_decode::{
    Visitor,
    visitor::{
        TypeIdFor,
        types::{Composite, Variant},
    },
};
use scale_type_resolver::TypeResolver;
use serde_json::Value as JsonValue;

/// An error we can encounter trying to decode things into a [`Value`]
#[derive(Debug, thiserror::Error)]
pub enum ValueError {
    #[error("Decode error: {0}")]
    Decode(#[from] scale_decode::visitor::DecodeError),
    #[error("Scale decode error: {0}")]
    ScaleDecodeError(#[from] scale_decode::Error),
    #[error("Cannot resolve variant type information: {0}")]
    CannotResolveVariantType(String),
}

impl From<ValueError> for scale_decode::Error {
    fn from(err: ValueError) -> Self {
        scale_decode::Error::new(scale_decode::error::ErrorKind::Custom(err.into()))
    }
}
#[derive(Debug)]
enum VariantFields {
    Unnamed(Vec<JsonValue>),
    Named(HashMap<String, JsonValue>),
}

fn to_json_fields<'r, 'scale, 'resolver, R: TypeResolver>(
    resolver: &'r R,
    value: &mut Composite<'scale, 'resolver, R>,
) -> Result<VariantFields, ValueError> {
    // If fields are unnamed, treat as array:
    if value.fields().iter().all(|f| f.name.is_none()) {
        return Ok(VariantFields::Unnamed(to_json_array(
            resolver,
            value.remaining(),
            value,
        )?));
    }

    // Otherwise object:
    let mut out = HashMap::new();
    for field in value {
        let field = field?;
        let name = field.name().unwrap_or("field").to_string();
        let value = field.decode_with_visitor(ToPjsOutputVisitor { resolver })?;
        out.insert(name, value);
    }
    Ok(VariantFields::Named(out))
}

fn to_json_array<'r, 'scale, 'resolver, R: TypeResolver>(
    resolver: &'r R,
    len: usize,
    mut values: impl scale_decode::visitor::DecodeItemIterator<'scale, 'resolver, R>,
) -> Result<Vec<JsonValue>, ValueError> {
    let mut out = Vec::with_capacity(len);
    while let Some(value) = values.decode_item(ToPjsOutputVisitor { resolver }) {
        out.push(value?);
    }
    Ok(out)
}

/// Visitor that transforms SCALE values to JSON with proper enum serialization
pub struct ToPjsOutputVisitor<'resolver, R> {
    resolver: &'resolver R,
}

impl<'resolver, R> ToPjsOutputVisitor<'resolver, R> {
    pub fn new(resolver: &'resolver R) -> Self {
        Self { resolver }
    }
}

impl<'resolver, R: TypeResolver> Visitor for ToPjsOutputVisitor<'resolver, R> {
    type Value<'scale, 'info> = JsonValue;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_variant<'scale, 'info>(
        self,
        value: &mut Variant<'scale, 'info, Self::TypeResolver>,
        type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        let variant_name = value.name();

        // Use resolver to check if ANY variant in this enum has data
        let variant_has_data_visitor = scale_type_resolver::visitor::new((), |_, _| false)
            .visit_variant(|_, _, variants| {
                let mut has_data = false;
                for mut variant in variants {
                    if variant.fields.next().is_some() {
                        has_data = true;
                    }
                }
                has_data
            });

        // Use this visitor to resolve the type information
        // Default to true (non-basic) if resolution fails - safer to use object format
        let variant_has_data = self
            .resolver
            .resolve_type(type_id, variant_has_data_visitor)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to resolve enum type for {}: {:?}, defaulting to non-basic", variant_name, e);
                true
            });

        // Base our decoding on whether any data in enum type.
        if variant_has_data {
            // Non-basic enum: serialize as {"lowercase": <data>}
            let fields = to_json_fields(self.resolver, value.fields())?;

            // Convert variant name to lowercase for the key
            let key = variant_name.to_lowercase();

            let values_json = match fields {
                VariantFields::Unnamed(values) if values.is_empty() => JsonValue::Null,
                VariantFields::Unnamed(values) => JsonValue::Array(values),
                VariantFields::Named(obj) => JsonValue::Object(obj.into_iter().collect()),
            };

            let mut result = serde_json::Map::new();
            result.insert(key, values_json);
            Ok(JsonValue::Object(result))
        } else {
            // Basic enum: serialize as string
            Ok(JsonValue::String(variant_name.to_string()))
        }
    }

    fn visit_composite<'scale, 'info>(
        self,
        value: &mut Composite<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        let fields_vec: Vec<_> = value.fields().into_iter().collect();
        let bytes = value.bytes_from_start();

        let all_unnamed = fields_vec.iter().all(|f| f.name.is_none());
        if all_unnamed && bytes.len() > 2 && !bytes.is_empty() && bytes.len() <= 256 {
            return Ok(JsonValue::String(format!("0x{}", hex::encode(bytes))));
        }
        // Decode composite fields normally
        let fields = to_json_fields(self.resolver, value)?;
        match fields {
            VariantFields::Named(obj) => Ok(JsonValue::Object(obj.into_iter().collect())),
            VariantFields::Unnamed(arr) => Ok(JsonValue::Array(arr)),
        }
    }

    fn visit_sequence<'scale, 'info>(
        self,
        value: &mut scale_decode::visitor::types::Sequence<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Array(to_json_array(
            self.resolver,
            value.remaining(),
            value,
        )?))
    }

    fn visit_array<'scale, 'info>(
        self,
        value: &mut scale_decode::visitor::types::Array<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Array(to_json_array(
            self.resolver,
            value.remaining(),
            value,
        )?))
    }

    fn visit_tuple<'scale, 'info>(
        self,
        value: &mut scale_decode::visitor::types::Tuple<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Array(to_json_array(
            self.resolver,
            value.remaining(),
            value,
        )?))
    }
    fn visit_str<'scale, 'info>(
        self,
        value: &mut scale_decode::visitor::types::Str<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::String(value.as_str()?.to_owned()))
    }

    fn visit_bitsequence<'scale, 'info>(
        self,
        value: &mut scale_decode::visitor::types::BitSequence<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        let bits = value.decode()?;
        let mut out = Vec::new();
        for bit in bits {
            let bit = bit.map_err(scale_decode::Error::from)?;
            out.push(JsonValue::Bool(bit));
        }
        Ok(JsonValue::Array(out))
    }

    fn visit_u8<'scale, 'info>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u16<'scale, 'info>(
        self,
        value: u16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u32<'scale, 'info>(
        self,
        value: u32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u64<'scale, 'info>(
        self,
        value: u64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u128<'scale, 'info>(
        self,
        value: u128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        // Convert large numbers to strings to preserve precision
        Ok(serde_json::json!(value.to_string()))
    }

    fn visit_i8<'scale, 'info>(
        self,
        value: i8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_i16<'scale, 'info>(
        self,
        value: i16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_i32<'scale, 'info>(
        self,
        value: i32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_i64<'scale, 'info>(
        self,
        value: i64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_i128<'scale, 'info>(
        self,
        value: i128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        // Convert large numbers to strings to preserve precision
        Ok(serde_json::json!(value.to_string()))
    }

    fn visit_bool<'scale, 'info>(
        self,
        value: bool,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_char<'scale, 'info>(
        self,
        value: char,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(serde_json::json!(value.to_string()))
    }

    fn visit_unexpected<'scale, 'info>(
        self,
        _unexpected: scale_decode::visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Null)
    }
}
