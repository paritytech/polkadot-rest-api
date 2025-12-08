use std::collections::HashMap;

/// Visitor for transforming SCALE-decoded values to JSON with proper enum serialization.
///
/// This visitor correctly handles the distinction between "basic" and "non-basic" enums
/// to match polkadot-js and substrate-api-sidecar serialization:
///
/// - **Basic enums** (all variants have no data): serialize as strings
///   Example: `DispatchClass::Normal` -> `"Normal"`
///
/// - **Non-basic enums** (at least one variant has data): serialize as objects
///   Example: `WeightLimit::Unlimited` -> `{"unlimited": null}`
///   Example: `WeightLimit::Limited(w)` -> `{"limited": <weight>}`
///
/// The visitor uses the type resolver to inspect the full enum type definition
/// and determine whether any variant has associated data.
use scale_decode::{
    Visitor,
    visitor::{
        DecodeItemIterator, TypeIdFor, Unexpected,
        types::{Array, BitSequence, Composite, Sequence, Str, Tuple, Variant},
    },
};
use scale_type_resolver::TypeResolver;
use scale_value::Value;
use serde_json::Value as JsonValue;

/// An error we can encounter trying to decode things into a [`Value`]
#[derive(Debug, thiserror::Error)]
pub enum ValueError {
    #[error("Decode error: {0}")]
    Decode(#[from] scale_decode::visitor::DecodeError),
    #[error("Cannot resolve variant type information: {0}")]
    CannotResolveVariantType(String),
}

#[derive(Debug, Clone)]
pub enum VariantFields {
    Unnamed(Vec<Value>),
    Named(HashMap<String, Value>),
}

#[derive(Debug, Clone)]
pub enum Value {
    Variant(String, VariantFields),
    VariantWithoutData(String),
}

/// Visitor that transforms SCALE values to JSON with proper enum serialization
pub struct ToPjsOutputVisitor<'resolver, R> {
    resolver: &'resolver R,
    /// Whether to convert addresses to SS58 format
    /// None = return as hex (for events)
    /// Some(prefix) = convert to SS58 (for extrinsic args)
    ss58_prefix: Option<u16>,
}

impl<'resolver, R> ToPjsOutputVisitor<'resolver, R> {
    pub fn new(resolver: &'resolver R, ss58_prefix: u16) -> Self {
        Self {
            resolver,
            ss58_prefix: Some(ss58_prefix),
        }
    }
}

impl<'resolver, R: TypeResolver> Visitor for ToPjsOutputVisitor<'resolver, R> {
    type Value<'scale, 'info> = Value;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_variant<'scale, 'info>(
        self,
        value: &mut Variant<'scale, 'info, Self::TypeResolver>,
        type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        // Use resolver to check if ANY variant in this enum has data
        let has_data_visitor =
            scale_type_resolver::visitor::new((), |_, _| false).visit_variant(|_, _, variants| {
                for variant in variants {
                    for _ in variant.fields {
                        return true;
                    }
                }
                false
            });

        // Resolve the enum TYPE to determine if it has data
        let has_data = self
            .resolver
            .resolve_type(type_id, has_data_visitor)
            .map_err(|e| ValueError::CannotResolveVariantType(e.to_string()))?;

        let variant_name = value.name();

        // base our decoding on whether any data in enum type.
        if has_data {
            let fields = to_variant_fieldish(self.resolver, value.fields())?;
            Ok(Value::VariantWithData(variant_name.to_string(), fields))
        } else {
            Ok(Value::VariantWithoutData(variant_name.to_string()))
        }
    }

    fn to_variant_fieldish<'r, 'scale, 'resolver, R: TypeResolver>(
        resolver: &'r R,
        value: &mut Composite<'scale, 'resolver, R>,
    ) -> Result<VariantFields, ValueError> {
        // If fields are unnamed, treat as array:
        if value.fields().iter().all(|f| f.name.is_none()) {
            return Ok(VariantFields::Unnamed(to_array(
                resolver,
                value.remaining(),
                value,
            )?));
        }

        // Otherwise object:
        let mut out = HashMap::new();
        for field in value {
            let field = field?;
            let name = field.name().unwrap().to_string();
            let value = field.decode_with_visitor(GetValue::new(resolver))?;
            out.insert(name, value);
        }
        Ok(VariantFields::Named(out))
    }

    fn visit_composite<'scale, 'info>(
        self,
        value: &mut Composite<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        // Special case: AccountId32 → hex or SS58
        if let Some(name) = value.name()
            && (name == "AccountId32" || name == "AccountId")
            && value.bytes_from_start().len() == 32
        {
            let bytes = value.bytes_from_start();
            let hex_string = format!("0x{}", hex::encode(bytes));

            // Convert to SS58 if ss58_prefix is provided
            if let Some(prefix) = self.ss58_prefix {
                if let Some(ss58_addr) = crate::utils::decode_address_to_ss58(&hex_string, prefix) {
                    return Ok(JsonValue::String(ss58_addr));
                }
            }

            return Ok(JsonValue::String(hex_string));
        }

        // Decode composite fields
        let fields = to_json_fields(self.resolver, self.ss58_prefix, value)?;

        match fields {
            JsonFields::Named(obj) => Ok(JsonValue::Object(obj)),
            JsonFields::Unnamed(arr) => Ok(JsonValue::Array(arr)),
        }
    }

    fn visit_sequence<'scale, 'info>(
        self,
        value: &mut Sequence<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Array(to_json_array(
            self.resolver,
            self.ss58_prefix,
            value.remaining(),
            value,
        )?))
    }

    fn visit_array<'scale, 'info>(
        self,
        value: &mut Array<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Array(to_json_array(
            self.resolver,
            self.ss58_prefix,
            value.remaining(),
            value,
        )?))
    }

    fn visit_tuple<'scale, 'info>(
        self,
        value: &mut Tuple<'scale, 'info, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Array(to_json_array(
            self.resolver,
            self.ss58_prefix,
            value.remaining(),
            value,
        )?))
    }

    fn visit_str<'scale, 'info>(
        self,
        value: &mut Str<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::String(value.as_str()?.to_owned()))
    }

    fn visit_bitsequence<'scale, 'info>(
        self,
        value: &mut BitSequence<'scale>,
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
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'info>, Self::Error> {
        Ok(JsonValue::Null)
    }
}

// ================================================================================================
// Helper Types and Functions
// ================================================================================================

/// Represents decoded fields that can be either named (object) or unnamed (array)
enum JsonFields {
    Named(serde_json::Map<String, JsonValue>),
    Unnamed(Vec<JsonValue>),
}

/// Decode composite/variant fields into JSON
///
/// If all fields are unnamed → returns array
/// If any field is named → returns object with field names as keys
fn to_json_fields<'r, 'scale, 'info, R: TypeResolver>(
    resolver: &'r R,
    ss58_prefix: Option<u16>,
    value: &mut Composite<'scale, 'info, R>,
) -> Result<JsonFields, scale_decode::Error> {
    // Check if all fields are unnamed
    let all_unnamed = value.fields().iter().all(|f| f.name.is_none());

    if all_unnamed {
        // Unnamed fields → array
        Ok(JsonFields::Unnamed(to_json_array(
            resolver,
            ss58_prefix,
            value.remaining(),
            value,
        )?))
    } else {
        // Named fields → object
        let mut out = serde_json::Map::new();
        for field in value {
            let field = field?;
            let name = field.name().unwrap_or("field").to_string();
            let value = field.decode_with_visitor(ToPjsOutputVisitor {
                resolver,
                ss58_prefix,
            })?;
            out.insert(name, value);
        }
        Ok(JsonFields::Named(out))
    }
}

/// Decode an iterator of items into a JSON array
fn to_json_array<'r, 'scale, 'info, R: TypeResolver>(
    resolver: &'r R,
    ss58_prefix: Option<u16>,
    len: usize,
    mut values: impl DecodeItemIterator<'scale, 'info, R>,
) -> Result<Vec<JsonValue>, scale_decode::Error> {
    let mut out = Vec::with_capacity(len);
    while let Some(value) = values.decode_item(ToPjsOutputVisitor {
        resolver,
        ss58_prefix,
    }) {
        out.push(value?);
    }
    Ok(out)
}
