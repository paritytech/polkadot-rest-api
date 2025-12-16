//! Type-aware JSON visitor for decoding extrinsic arguments.
//!
//! This module provides `JsonVisitor`, a visitor that decodes SCALE values directly
//! to JSON with type-aware transformations. It handles SS58 encoding for account types,
//! byte array conversion, and proper array/newtype handling.
//!
//! For enum types, it distinguishes between "basic" enums (all variants have no data)
//! and "non-basic" enums (any variant has data):
//! - Basic enums serialize as strings: `"Normal"`, `"Yes"`
//! - Non-basic enums serialize as objects: `{"unlimited": null}`, `{"limited": {...}}`

use heck::ToLowerCamelCase;
use scale_decode::visitor::{self, TypeIdFor};
use scale_type_resolver::TypeResolver;
use serde_json::Value;
use sp_core::crypto::{AccountId32, Ss58Codec};

/// Check if an enum type is "basic" (all variants have no associated data).
///
/// Basic enums should serialize as strings (e.g., `"Normal"`, `"Yes"`),
/// while non-basic enums serialize as objects (e.g., `{"unlimited": null}`).
///
/// This determination is made at the TYPE level, not the variant level.
/// For example, `WeightLimit::Unlimited` has no data, but `WeightLimit` is
/// non-basic because `Limited(Weight)` has data.
fn is_basic_enum<R: TypeResolver>(resolver: &R, type_id: R::TypeId) -> bool
where
    R::TypeId: Clone,
{
    let type_visitor =
        scale_type_resolver::visitor::new((), |_, _| false).visit_variant(|_, _, variants| {
            // Check if ANY variant has fields - if so, NOT basic
            for variant in variants {
                if variant.fields.len() > 0 {
                    return false;
                }
            }
            true // All variants have no fields = IS basic
        });

    resolver
        .resolve_type(type_id, type_visitor)
        .unwrap_or(false)
}

/// Check if variant name is an X2-X8 junction.
/// These variants need special handling to preserve array output format.
/// Note: X1 is intentionally excluded - it has a single element and should be
/// unwrapped like any other single-field variant to match sidecar's output format.
fn is_junction_variant(name: &str) -> bool {
    matches!(name, "X2" | "X3" | "X4" | "X5" | "X6" | "X7" | "X8")
}

/// A visitor that decodes SCALE values directly to JSON with type-aware transformations.
///
/// This handles:
/// - SS58 encoding ONLY for AccountId32/MultiAddress/AccountId types (not hashes)
/// - Preserving arrays for sequence types (Vec<T>) - never unwraps single-element sequences
/// - Unwrapping newtype wrappers (single unnamed field composites)
/// - Converting byte arrays to hex strings
/// - Basic enums as strings (e.g., `"Normal"`), non-basic enums as objects (e.g., `{"unlimited": null}`)
/// - Converting all numbers to strings (matching sidecar behavior)
///
/// The key advantage over post-processing transformations is that this visitor has access to
/// type information at every nesting level, allowing it to make correct decisions about
/// SS58 encoding and array unwrapping.
pub struct JsonVisitor<'r, R> {
    ss58_prefix: u16,
    resolver: &'r R,
}

impl<'r, R> JsonVisitor<'r, R> {
    pub fn new(ss58_prefix: u16, resolver: &'r R) -> Self {
        JsonVisitor {
            ss58_prefix,
            resolver,
        }
    }
}

impl<'r, R> scale_decode::Visitor for JsonVisitor<'r, R>
where
    R: scale_type_resolver::TypeResolver,
    R::TypeId: Clone,
{
    type Value<'scale, 'resolver> = Value;
    type Error = scale_decode::Error;
    type TypeResolver = R;

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
        // Convert to hex for u256
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
        while let Some(item) = value.decode_item(JsonVisitor::new(self.ss58_prefix, self.resolver))
        {
            items.push(item?);
        }

        // Check if this is a Vec<u8> - all items are string representations of bytes
        if items.len() >= 2 {
            let mut is_byte_sequence = true;
            let mut bytes = Vec::with_capacity(items.len());

            for item in &items {
                if let Value::String(s) = item
                    && let Ok(n) = s.parse::<u64>()
                    && n <= 255
                {
                    bytes.push(n as u8);
                    continue;
                }
                is_byte_sequence = false;
                break;
            }

            if is_byte_sequence && bytes.len() == items.len() {
                return Ok(Value::String(format!("0x{}", hex::encode(&bytes))));
            }
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

        // If it's an account type, try to extract bytes and convert to SS58
        // Otherwise fall through to regular composite handling
        if is_account_type {
            let mut bytes = Vec::new();
            let field_count = value.remaining();

            // For AccountId32, it's typically a single unnamed field containing 32 bytes
            // For MultiAddress, it's an enum (handled in visit_variant)
            if field_count > 0 {
                for field in value.by_ref() {
                    let field = field?;
                    match field.decode_with_visitor(ByteCollector::<R>::new()) {
                        Ok(field_bytes) => bytes.extend(field_bytes),
                        Err(_) => {
                            bytes.clear();
                            break;
                        }
                    }
                }
            }

            // If we got exactly 32 bytes, convert to SS58
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                let account_id = AccountId32::from(arr);
                let ss58 = account_id.to_ss58check_with_version(self.ss58_prefix.into());
                return Ok(Value::String(ss58));
            }
        }

        // Check if all fields are unnamed (tuple/array-like)
        let fields: Vec<_> = value.collect::<Result<Vec<_>, _>>()?;

        if fields.is_empty() {
            return Ok(Value::Null);
        }

        // We check if the field is named or unnamed
        if fields[0].name().is_some() {
            // Deal with named fields and return a JSON object
            let mut map = serde_json::Map::new();
            for field in fields {
                if let Some(name) = field.name() {
                    let key = name.to_lower_camel_case();
                    let val = field
                        .decode_with_visitor(JsonVisitor::new(self.ss58_prefix, self.resolver))?;
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
                    match field
                        .clone()
                        .decode_with_visitor(ByteValueVisitor::<R>::new())
                    {
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

            // We deal with a single unnamed field.
            // Note: We already handled sequences in visit_sequence, so this is safe
            if field_count == 1 {
                if let Some(field) = fields.into_iter().next() {
                    return field
                        .decode_with_visitor(JsonVisitor::new(self.ss58_prefix, self.resolver));
                }
                // Fallback: return empty array if iterator unexpectedly empty
                return Ok(Value::Array(vec![]));
            }

            // Deal with multiple unnamed fields
            let arr: Result<Vec<_>, _> = fields
                .into_iter()
                .map(|f| f.decode_with_visitor(JsonVisitor::new(self.ss58_prefix, self.resolver)))
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

        // Handle Option::None as JSON null
        if name == "None" {
            // Consume the fields even though we're returning null
            for field in value.fields() {
                field?.decode_with_visitor(SkipVisitor::<R>::new())?;
            }
            return Ok(Value::Null);
        }

        // Handle Option::Some - unwrap and return just the inner value (not {"some": value})
        if name == "Some" {
            let fields: Vec<_> = value.fields().collect::<Result<Vec<_>, _>>()?;
            if fields.len() == 1
                && let Some(field) = fields.into_iter().next()
            {
                return field
                    .decode_with_visitor(JsonVisitor::new(self.ss58_prefix, self.resolver));
            }
            // Fallback for unexpected Some structure
            return Ok(Value::Null);
        }

        // Convert variant name, ex: "PreRuntime" -> "preRuntime"
        let variant_name = crate::utils::lowercase_first_char(name);

        // Check if this is a basic enum (all variants have no data).
        // Basic enums serialize as strings: "Normal", "Yes"
        // Non-basic enums serialize as objects: {"unlimited": null}
        if is_basic_enum(self.resolver, type_id) {
            // Consume any fields (there shouldn't be any for basic enum variants)
            for field in value.fields() {
                field?.decode_with_visitor(SkipVisitor::<R>::new())?;
            }
            return Ok(Value::String(variant_name));
        }

        // Non-basic enum - wrap variant in an object
        let is_junction = is_junction_variant(name);
        let fields: Vec<_> = value.fields().collect::<Result<Vec<_>, _>>()?;

        let inner = if fields.is_empty() {
            Value::Null
        } else if fields[0].name().is_some() {
            // Deal with named fields
            let mut map = serde_json::Map::new();
            for field in fields {
                if let Some(name) = field.name() {
                    let key = name.to_lower_camel_case();
                    let val = field
                        .decode_with_visitor(JsonVisitor::new(self.ss58_prefix, self.resolver))?;
                    map.insert(key, val);
                }
            }
            Value::Object(map)
        } else if fields.len() == 1 && !is_junction {
            // Deal with a single unnamed field
            match fields.into_iter().next() {
                Some(field) => {
                    field.decode_with_visitor(JsonVisitor::new(self.ss58_prefix, self.resolver))?
                }
                None => Value::Array(vec![]), // Fallback
            }
        } else {
            // Deal with multiple unnamed fields or Junction types
            let arr: Result<Vec<_>, _> = fields
                .into_iter()
                .map(|f| f.decode_with_visitor(JsonVisitor::new(self.ss58_prefix, self.resolver)))
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
        // Decode all elements first with JsonVisitor
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(JsonVisitor::new(self.ss58_prefix, self.resolver))
        {
            items.push(item?);
        }

        // Check if all items are string representations of u8 values (0-255)
        // This happens when we have a fixed-size byte array [u8; N]
        if items.len() >= 2 {
            let mut is_byte_array = true;
            let mut bytes = Vec::with_capacity(items.len());

            for item in &items {
                if let Value::String(s) = item
                    && let Ok(n) = s.parse::<u64>()
                    && n <= 255
                {
                    bytes.push(n as u8);
                    continue;
                }
                is_byte_array = false;
                break;
            }

            if is_byte_array && bytes.len() == items.len() {
                return Ok(Value::String(format!("0x{}", hex::encode(&bytes))));
            }
        }

        Ok(Value::Array(items))
    }

    fn visit_tuple<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Tuple<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(JsonVisitor::new(self.ss58_prefix, self.resolver))
        {
            items.push(item?);
        }

        if items.len() == 1 {
            if let Some(item) = items.into_iter().next() {
                return Ok(item);
            }
            // Fallback: return empty array if iterator unexpectedly empty
            return Ok(Value::Array(vec![]));
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

/// Helper visitor that collects bytes from a composite (for AccountId32 extraction)
struct ByteCollector<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> ByteCollector<R> {
    fn new() -> Self {
        ByteCollector {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> scale_decode::Visitor for ByteCollector<R>
where
    R: scale_type_resolver::TypeResolver,
{
    type Value<'scale, 'resolver> = Vec<u8>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

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
        while let Some(item) = value.decode_item(ByteCollector::<R>::new()) {
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
            bytes.extend(field.decode_with_visitor(ByteCollector::<R>::new())?);
        }
        Ok(bytes)
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Return empty vec instead of error for unexpected types
        Ok(Vec::new())
    }
}

/// Helper visitor that checks if a value is a single u8 byte
struct ByteValueVisitor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> ByteValueVisitor<R> {
    fn new() -> Self {
        ByteValueVisitor {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> scale_decode::Visitor for ByteValueVisitor<R>
where
    R: scale_type_resolver::TypeResolver,
{
    type Value<'scale, 'resolver> = Option<u8>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

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

/// Helper visitor that skips/ignores a value (for consuming Option::None fields)
struct SkipVisitor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> SkipVisitor<R> {
    fn new() -> Self {
        SkipVisitor {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> scale_decode::Visitor for SkipVisitor<R>
where
    R: scale_type_resolver::TypeResolver,
{
    type Value<'scale, 'resolver> = ();
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(())
    }
}
