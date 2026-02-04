//! Type-aware JSON visitor for decoding extrinsic arguments.
//!
//! This module provides `ScaleVisitor`, a generic visitor that decodes SCALE values
//! directly to JSON with type-aware transformations. The const generic `CAMEL_CASE`
//! controls whether field names are converted to camelCase or kept as snake_case.
//!
//! Type aliases:
//! - `JsonVisitor`: Uses camelCase for field names (general API responses)
//! - `CallArgsVisitor`: Keeps snake_case for field names (extrinsic call args, matching sidecar)
//!
//! For enum types, it distinguishes between "basic" enums (all variants have no data)
//! and "non-basic" enums (any variant has data):
//! - Basic enums serialize as strings: `"Normal"`, `"Yes"`
//! - Non-basic enums serialize as objects: `{"unlimited": null}`, `{"limited": {...}}`

use heck::ToLowerCamelCase;
use scale_decode::visitor::{self, TypeIdFor};
use scale_info::PortableRegistry;
use scale_type_resolver::TypeResolver;
use serde_json::Value;
use sp_core::crypto::{AccountId32, Ss58Codec};

/// Type alias for the visitor that converts field names to camelCase.
pub type JsonVisitor<'r> = ScaleVisitor<'r, true>;

/// Type alias for the visitor that keeps field names in snake_case.
pub type CallArgsVisitor<'r> = ScaleVisitor<'r, false>;

/// Check if an enum type is "basic" (all variants have no associated data).
fn is_basic_enum(resolver: &PortableRegistry, type_id: u32) -> bool {
    let type_visitor =
        scale_type_resolver::visitor::new((), |_, _| false).visit_variant(|_, _, variants| {
            for variant in variants {
                if variant.fields.len() > 0 {
                    return false;
                }
            }
            true
        });

    resolver
        .resolve_type(type_id, type_visitor)
        .unwrap_or(false)
}

/// Check if variant name is an X2-X8 junction (need special array handling).
fn is_junction_variant(name: &str) -> bool {
    matches!(name, "X2" | "X3" | "X4" | "X5" | "X6" | "X7" | "X8")
}

fn is_call_type(resolver: &PortableRegistry, type_id: u32) -> bool {
    if let Some(ty) = resolver.resolve(type_id) {
        for segment in ty.path.segments.iter() {
            if segment.ends_with("Call") {
                return true;
            }
        }
    }
    false
}

#[inline]
fn format_field_name<const CAMEL_CASE: bool>(name: &str) -> String {
    if CAMEL_CASE {
        name.to_lower_camel_case()
    } else {
        name.to_string()
    }
}

fn try_items_to_hex(items: &[Value]) -> Option<String> {
    if items.len() < 2 {
        return None;
    }

    let mut bytes = Vec::with_capacity(items.len());
    for item in items {
        if let Value::String(s) = item
            && let Ok(n) = s.parse::<u64>()
            && n <= 255
        {
            bytes.push(n as u8);
            continue;
        }
        return None;
    }

    Some(format!("0x{}", hex::encode(&bytes)))
}

/// A generic visitor that decodes SCALE values to JSON.
///
/// - `CAMEL_CASE = true`: Convert field names to camelCase (use `JsonVisitor` alias)
/// - `CAMEL_CASE = false`: Keep field names in snake_case (use `CallArgsVisitor` alias)
pub struct ScaleVisitor<'r, const CAMEL_CASE: bool> {
    ss58_prefix: u16,
    resolver: &'r PortableRegistry,
}

impl<'r, const CAMEL_CASE: bool> ScaleVisitor<'r, CAMEL_CASE> {
    pub fn new(ss58_prefix: u16, resolver: &'r PortableRegistry) -> Self {
        Self {
            ss58_prefix,
            resolver,
        }
    }

    fn child(&self) -> Self {
        Self::new(self.ss58_prefix, self.resolver)
    }
}

impl<'r, const CAMEL_CASE: bool> scale_decode::Visitor for ScaleVisitor<'r, CAMEL_CASE> {
    type Value<'scale, 'resolver> = Value;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_bool<'scale, 'resolver>(
        self,
        value: bool,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::Bool(value))
    }

    fn visit_char<'scale, 'resolver>(
        self,
        value: char,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u16<'scale, 'resolver>(
        self,
        value: u16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u32<'scale, 'resolver>(
        self,
        value: u32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u64<'scale, 'resolver>(
        self,
        value: u64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u128<'scale, 'resolver>(
        self,
        value: u128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u256<'resolver>(
        self,
        value: &[u8; 32],
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'_, 'resolver>, Self::Error> {
        Ok(Value::String(format!("0x{}", hex::encode(value))))
    }

    fn visit_i8<'scale, 'resolver>(
        self,
        value: i8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i16<'scale, 'resolver>(
        self,
        value: i16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i32<'scale, 'resolver>(
        self,
        value: i32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i64<'scale, 'resolver>(
        self,
        value: i64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i128<'scale, 'resolver>(
        self,
        value: i128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i256<'resolver>(
        self,
        value: &[u8; 32],
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'_, 'resolver>, Self::Error> {
        Ok(Value::String(format!("0x{}", hex::encode(value))))
    }

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(self.child()) {
            items.push(item?);
        }

        if let Some(hex) = try_items_to_hex(&items) {
            return Ok(Value::String(hex));
        }

        Ok(Value::Array(items))
    }

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let path_segments: Vec<_> = value.path().collect();
        let is_account_type = path_segments
            .iter()
            .any(|s| *s == "AccountId32" || *s == "MultiAddress" || *s == "AccountId");
        let is_vote_type = path_segments.contains(&"Vote");

        if is_account_type && let Some(ss58) = self.try_extract_ss58(value)? {
            return Ok(Value::String(ss58));
        }

        if is_vote_type && let Some(byte) = self.try_extract_single_byte(value)? {
            return Ok(Value::String(format!("0x{:02x}", byte)));
        }

        let fields: Vec<_> = value.collect::<Result<Vec<_>, _>>()?;

        if fields.is_empty() {
            return Ok(Value::Null);
        }

        if fields[0].name().is_some() {
            let mut map = serde_json::Map::new();
            for field in fields {
                if let Some(name) = field.name() {
                    let key = format_field_name::<CAMEL_CASE>(name);
                    let val = field.decode_with_visitor(self.child())?;
                    map.insert(key, val);
                }
            }
            Ok(Value::Object(map))
        } else {
            let field_count = fields.len();
            if field_count >= 2 {
                let mut is_byte_array = true;
                let mut bytes = Vec::with_capacity(field_count);

                for field in &fields {
                    match field.clone().decode_with_visitor(ByteValueVisitor) {
                        Ok(Some(byte)) => bytes.push(byte),
                        _ => {
                            is_byte_array = false;
                            break;
                        }
                    }
                }

                if is_byte_array && bytes.len() == field_count {
                    return Ok(Value::String(format!("0x{}", hex::encode(&bytes))));
                }
            }

            if field_count == 1 {
                if let Some(field) = fields.into_iter().next() {
                    return field.decode_with_visitor(self.child());
                }
                return Ok(Value::Array(vec![]));
            }

            let arr: Result<Vec<_>, _> = fields
                .into_iter()
                .map(|f| f.decode_with_visitor(self.child()))
                .collect();
            Ok(Value::Array(arr?))
        }
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Variant<'scale, 'resolver, Self::TypeResolver>,
        type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let name = value.name();

        if name == "None" {
            for field in value.fields() {
                field?.decode_with_visitor(SkipVisitor)?;
            }
            return Ok(Value::Null);
        }

        if name == "Some" {
            let fields: Vec<_> = value.fields().collect::<Result<Vec<_>, _>>()?;
            if fields.len() == 1
                && let Some(field) = fields.into_iter().next()
            {
                return field.decode_with_visitor(self.child());
            }
            return Ok(Value::Null);
        }

        if is_call_type(self.resolver, type_id) {
            return self.decode_call_variant(value);
        }

        let variant_name = crate::utils::lowercase_first_char(name);

        if is_basic_enum(self.resolver, type_id) {
            for field in value.fields() {
                field?.decode_with_visitor(SkipVisitor)?;
            }
            return Ok(Value::String(variant_name));
        }

        let is_junction = is_junction_variant(name);
        let fields: Vec<_> = value.fields().collect::<Result<Vec<_>, _>>()?;

        let inner = if fields.is_empty() {
            Value::Null
        } else if fields[0].name().is_some() {
            let mut map = serde_json::Map::new();
            for field in fields {
                if let Some(name) = field.name() {
                    let key = format_field_name::<CAMEL_CASE>(name);
                    let val = field.decode_with_visitor(self.child())?;
                    map.insert(key, val);
                }
            }
            Value::Object(map)
        } else if fields.len() == 1 && !is_junction {
            fields
                .into_iter()
                .next()
                .map(|f| f.decode_with_visitor(self.child()))
                .transpose()?
                .unwrap_or(Value::Array(vec![]))
        } else {
            let arr: Result<Vec<_>, _> = fields
                .into_iter()
                .map(|f| f.decode_with_visitor(self.child()))
                .collect();
            Value::Array(arr?)
        };

        let mut map = serde_json::Map::new();
        map.insert(variant_name, inner);
        Ok(Value::Object(map))
    }

    fn visit_array<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Array<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(self.child()) {
            items.push(item?);
        }

        if let Some(hex) = try_items_to_hex(&items) {
            return Ok(Value::String(hex));
        }

        Ok(Value::Array(items))
    }

    fn visit_tuple<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Tuple<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(self.child()) {
            items.push(item?);
        }

        if items.len() == 1 {
            return Ok(items.into_iter().next().unwrap_or(Value::Array(vec![])));
        }

        Ok(Value::Array(items))
    }

    fn visit_str<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Str<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.as_str()?.to_string()))
    }

    fn visit_bitsequence<'scale, 'resolver>(
        self,
        value: &mut visitor::types::BitSequence<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let bits: Vec<bool> = value
            .decode()?
            .collect::<Result<Vec<_>, _>>()
            .map_err(scale_decode::Error::custom)?;
        let bytes: Vec<u8> = bits
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0u8, |acc, (i, &bit)| acc | ((bit as u8) << i))
            })
            .collect();
        Ok(Value::String(format!("0x{}", hex::encode(bytes))))
    }
}

impl<'r, const CAMEL_CASE: bool> ScaleVisitor<'r, CAMEL_CASE> {
    fn try_extract_single_byte(
        &self,
        value: &mut visitor::types::Composite<'_, '_, PortableRegistry>,
    ) -> Result<Option<u8>, scale_decode::Error> {
        if value.remaining() == 1 {
            for field in value.by_ref() {
                let field = field?;
                if let Ok(Some(byte)) = field.decode_with_visitor(ByteValueVisitor) {
                    return Ok(Some(byte));
                }
            }
        }
        Ok(None)
    }

    fn try_extract_ss58(
        &self,
        value: &mut visitor::types::Composite<'_, '_, PortableRegistry>,
    ) -> Result<Option<String>, scale_decode::Error> {
        let mut bytes = Vec::new();

        if value.remaining() > 0 {
            for field in value.by_ref() {
                let field = field?;
                match field.decode_with_visitor(ByteCollector) {
                    Ok(field_bytes) => bytes.extend(field_bytes),
                    Err(_) => {
                        bytes.clear();
                        break;
                    }
                }
            }
        }

        if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            let account_id = AccountId32::from(arr);
            let ss58 = account_id.to_ss58check_with_version(self.ss58_prefix.into());
            return Ok(Some(ss58));
        }

        Ok(None)
    }

    fn decode_call_variant<'scale, 'resolver>(
        &self,
        value: &mut visitor::types::Variant<'scale, 'resolver, PortableRegistry>,
    ) -> Result<Value, scale_decode::Error> {
        let variant_name = value.name();
        let fields: Vec<_> = value.fields().collect::<Result<Vec<_>, _>>()?;

        if fields.len() == 1 && fields[0].name().is_none() {
            let pallet_name = heck::AsSnakeCase(variant_name).to_string();
            let inner_value = fields
                .into_iter()
                .next()
                .unwrap()
                .decode_with_visitor(CallArgsVisitor::new(self.ss58_prefix, self.resolver))?;

            if let Value::Object(mut inner_map) = inner_value {
                if let Some(Value::Object(method_obj)) = inner_map.get("method") {
                    let mut result = serde_json::Map::new();
                    let mut method_map = serde_json::Map::new();
                    method_map.insert("pallet".to_string(), Value::String(pallet_name));
                    if let Some(method_name) = method_obj.get("method") {
                        method_map.insert("method".to_string(), method_name.clone());
                    }
                    result.insert("method".to_string(), Value::Object(method_map));
                    if let Some(args) = inner_map.remove("args") {
                        result.insert("args".to_string(), args);
                    }
                    return Ok(Value::Object(result));
                }
                let mut result = serde_json::Map::new();
                let mut method_map = serde_json::Map::new();
                method_map.insert("pallet".to_string(), Value::String(pallet_name));
                result.insert("method".to_string(), Value::Object(method_map));
                result.insert("args".to_string(), Value::Object(inner_map));
                return Ok(Value::Object(result));
            }

            let mut result = serde_json::Map::new();
            let mut method_map = serde_json::Map::new();
            method_map.insert("pallet".to_string(), Value::String(pallet_name));
            result.insert("method".to_string(), Value::Object(method_map));
            return Ok(Value::Object(result));
        }

        let method_name = variant_name.to_lower_camel_case();

        let args = if fields.is_empty() {
            Value::Object(serde_json::Map::new())
        } else if fields[0].name().is_some() {
            let mut args_map = serde_json::Map::new();
            for field in fields {
                if let Some(name) = field.name() {
                    let key = name.to_string();
                    let val = field.decode_with_visitor(CallArgsVisitor::new(
                        self.ss58_prefix,
                        self.resolver,
                    ))?;
                    args_map.insert(key, val);
                }
            }
            Value::Object(args_map)
        } else {
            let arr: Result<Vec<_>, _> = fields
                .into_iter()
                .map(|f| {
                    f.decode_with_visitor(CallArgsVisitor::new(self.ss58_prefix, self.resolver))
                })
                .collect();
            Value::Array(arr?)
        };

        let mut result = serde_json::Map::new();
        let mut method_map = serde_json::Map::new();
        method_map.insert("method".to_string(), Value::String(method_name));
        result.insert("method".to_string(), Value::Object(method_map));
        result.insert("args".to_string(), args);
        Ok(Value::Object(result))
    }
}

struct ByteCollector;

impl scale_decode::Visitor for ByteCollector {
    type Value<'scale, 'resolver> = Vec<u8>;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(vec![value])
    }

    fn visit_array<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Array<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut bytes = Vec::new();
        while let Some(item) = value.decode_item(ByteCollector) {
            bytes.extend(item?);
        }
        Ok(bytes)
    }

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut bytes = Vec::new();
        for field in value {
            let field = field?;
            bytes.extend(field.decode_with_visitor(ByteCollector)?);
        }
        Ok(bytes)
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Vec::new())
    }
}

/// Helper visitor that checks if a value is a single u8 byte.
struct ByteValueVisitor;

impl scale_decode::Visitor for ByteValueVisitor {
    type Value<'scale, 'resolver> = Option<u8>;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Some(value))
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(None)
    }
}

struct SkipVisitor;

impl scale_decode::Visitor for SkipVisitor {
    type Value<'scale, 'resolver> = ();
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(())
    }
}
