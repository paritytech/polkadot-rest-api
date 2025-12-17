//! Event decoding and transformation for block data.
//!
//! This module provides:
//! - `EventsVisitor` for extracting event information from System.Events storage
//! - Post-processing functions for transforming decoded event data to JSON
//!
//! For enum types, it distinguishes between "basic" enums (all variants have no data)
//! and "non-basic" enums (any variant has data):
//! - Basic enums serialize as strings: `"Normal"`, `"Yes"`
//! - Non-basic enums serialize as objects: `{"unlimited": null}`, `{"limited": {...}}`

use heck::ToLowerCamelCase;
use scale_decode::{
    Visitor,
    visitor::{
        TypeIdFor, Unexpected,
        types::{Composite, Sequence, Variant},
    },
};
use scale_type_resolver::TypeResolver;
use serde_json::Value as JsonValue;
use sp_core::crypto::{AccountId32, Ss58Codec};

/// Check if an enum type is "basic" (all variants have no associated data).
///
/// Basic enums should serialize as strings (e.g., `"Normal"`, `"Yes"`),
/// while non-basic enums serialize as objects (e.g., `{"unlimited": null}`).
///
/// This determination is made at the TYPE level, not the variant level.
fn is_basic_enum<R: TypeResolver>(resolver: &R, type_id: R::TypeId) -> bool
where
    R::TypeId: Clone,
{
    let type_visitor =
        scale_type_resolver::visitor::new((), |_, _| false).visit_variant(|_, _path, variants| {
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

// ================================================================================================
// Event Visitor Types
// ================================================================================================

/// Lowercase the first character only, preserving the rest
/// e.g., "ParaInclusion" -> "paraInclusion", "System" -> "system"
fn lowercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

/// Represents a single event field with its type name and value
#[derive(Debug, Clone)]
pub struct EventField {
    pub type_name: Option<String>,
    pub value: JsonValue,
}

/// Represents a single event with its metadata and field type information
#[derive(Debug, Clone)]
pub struct EventInfo {
    pub phase: EventPhase,
    pub pallet_name: String,
    pub event_name: String,
    pub fields: Vec<EventField>,
}

/// Event phase extracted from EventRecord
#[derive(Debug, Clone)]
pub enum EventPhase {
    Initialization,
    ApplyExtrinsic(u32),
    Finalization,
}

/// Visitor that collects all events with their field type information
pub struct EventsVisitor<'r, R> {
    resolver: &'r R,
}

impl<'r, R> EventsVisitor<'r, R> {
    pub fn new(resolver: &'r R) -> Self {
        Self { resolver }
    }
}

impl<'r, R> Visitor for EventsVisitor<'r, R>
where
    R: TypeResolver,
    R::TypeId: Clone,
{
    type Value<'scale, 'resolver> = Vec<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut events = Vec::new();

        // Iterate through each EventRecord in the Vec
        while let Some(event_record_result) =
            value.decode_item(EventRecordVisitor::new(self.resolver))
        {
            match event_record_result {
                Ok(Some(event_info)) => events.push(event_info),
                Ok(None) => {
                    // Skip events we couldn't parse
                    tracing::debug!("Skipped unparseable event");
                }
                Err(e) => {
                    tracing::warn!("Failed to decode event record: {:?}", e);
                }
            }
        }

        Ok(events)
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Err(parity_scale_codec::Error::from("Expected sequence for events").into())
    }
}

/// Visitor for a single EventRecord (composite with phase, event, topics)
struct EventRecordVisitor<'r, R> {
    resolver: &'r R,
}

impl<'r, R> EventRecordVisitor<'r, R> {
    fn new(resolver: &'r R) -> Self {
        Self { resolver }
    }
}

impl<'r, R> Visitor for EventRecordVisitor<'r, R>
where
    R: TypeResolver,
    R::TypeId: Clone,
{
    type Value<'scale, 'resolver> = Option<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // EventRecord has 3 fields: phase (0), event (1), topics (2)

        // Field 0: Extract phase
        let phase = if let Some(phase_result) = value.decode_item(PhaseExtractor::new()) {
            phase_result?
        } else {
            EventPhase::Finalization // Default fallback
        };

        // Field 1: Get the actual event
        if let Some(event_result) = value.decode_item(PalletEventVisitor::new(phase, self.resolver))
        {
            return event_result;
        }

        Ok(None)
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(None)
    }
}

/// Visitor that extracts the phase from EventRecord
struct PhaseExtractor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> PhaseExtractor<R> {
    fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for PhaseExtractor<R>
where
    R: TypeResolver,
{
    type Value<'scale, 'resolver> = EventPhase;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let variant_name = value.name();
        let fields = value.fields();

        match variant_name {
            "ApplyExtrinsic" => {
                // Extract the extrinsic index (u32)
                if let Some(index_result) = fields.decode_item(U32Extractor::new()) {
                    Ok(EventPhase::ApplyExtrinsic(index_result?))
                } else {
                    Ok(EventPhase::ApplyExtrinsic(0))
                }
            }
            "Initialization" => Ok(EventPhase::Initialization),
            "Finalization" => Ok(EventPhase::Finalization),
            _ => {
                tracing::warn!("Unknown phase variant: {}", variant_name);
                Ok(EventPhase::Finalization)
            }
        }
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(EventPhase::Finalization)
    }
}

/// Helper visitor to extract u32 values
struct U32Extractor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> U32Extractor<R> {
    fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for U32Extractor<R>
where
    R: TypeResolver,
{
    type Value<'scale, 'resolver> = u32;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_u32<'scale, 'resolver>(
        self,
        value: u32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(value)
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(0)
    }
}

/// Visitor for the pallet-level variant (e.g., Balances, System, etc.)
struct PalletEventVisitor<'r, R> {
    phase: EventPhase,
    resolver: &'r R,
}

impl<'r, R> PalletEventVisitor<'r, R> {
    fn new(phase: EventPhase, resolver: &'r R) -> Self {
        Self { phase, resolver }
    }
}

impl<'r, R> Visitor for PalletEventVisitor<'r, R>
where
    R: TypeResolver,
    R::TypeId: Clone,
{
    type Value<'scale, 'resolver> = Option<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // The variant name is the pallet name (e.g., "Balances", "System")
        // Lowercase the first char to match substrate-api-sidecar format
        let pallet_name = lowercase_first_char(value.name());

        // The variant contains fields - get the composite to access them
        let fields_composite = value.fields();

        // The first field should be the inner event variant
        if let Some(inner_event_result) = fields_composite.decode_item(ActualEventVisitor::new(
            self.phase,
            pallet_name,
            self.resolver,
        )) {
            return inner_event_result;
        }

        Ok(None)
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(None)
    }
}

/// Visitor for the actual event variant (e.g., Transfer, Withdraw, etc.)
struct ActualEventVisitor<'r, R> {
    phase: EventPhase,
    pallet_name: String,
    resolver: &'r R,
}

impl<'r, R> ActualEventVisitor<'r, R> {
    fn new(phase: EventPhase, pallet_name: String, resolver: &'r R) -> Self {
        Self {
            phase,
            pallet_name,
            resolver,
        }
    }
}

impl<'r, R> Visitor for ActualEventVisitor<'r, R>
where
    R: TypeResolver,
    R::TypeId: Clone,
{
    type Value<'scale, 'resolver> = Option<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let event_name = value.name().to_string();
        let mut event_fields = Vec::new();

        // Get the fields composite
        let fields_composite = value.fields();

        // Decode each field and extract both type name and value
        while let Some(field_result) =
            fields_composite.decode_item(FieldWithTypeExtractor::new(self.resolver))
        {
            match field_result {
                Ok((type_name, json_value)) => {
                    event_fields.push(EventField {
                        type_name,
                        value: json_value,
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to decode field: {:?}", e);
                }
            }
        }

        Ok(Some(EventInfo {
            phase: self.phase,
            pallet_name: self.pallet_name,
            event_name,
            fields: event_fields,
        }))
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(None)
    }
}

/// Visitor that extracts both the type name and JSON value for a field
struct FieldWithTypeExtractor<'r, R> {
    resolver: &'r R,
}

impl<'r, R> FieldWithTypeExtractor<'r, R> {
    fn new(resolver: &'r R) -> Self {
        Self { resolver }
    }
}

impl<'r, R> Visitor for FieldWithTypeExtractor<'r, R>
where
    R: TypeResolver,
    R::TypeId: Clone,
{
    type Value<'scale, 'resolver> = (Option<String>, JsonValue);
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Get type name from path
        let type_name = value.path().last().map(|s| s.to_string());

        // Special handling for AccountId32 - return as hex string instead of decoding fields
        // Only do this if the bytes are EXACTLY 32 (a raw AccountId32), not larger composite structures
        if type_name.as_deref() == Some("AccountId32") || type_name.as_deref() == Some("AccountId")
        {
            let bytes = value.bytes_from_start();
            if bytes.len() == 32 {
                let hex_string = format!("0x{}", hex::encode(bytes));
                return Ok((type_name, JsonValue::String(hex_string)));
            }
        }

        // Collect field names before decoding (since decode_item consumes them)
        let field_names: Vec<Option<String>> = value
            .fields()
            .iter()
            .map(|f| f.name.map(|s| s.to_lower_camel_case()))
            .collect();
        let has_named_fields = field_names.iter().any(|n| n.is_some());

        let mut field_values = Vec::new();
        while let Some(field_result) = value.decode_item(ValueExtractor::new(self.resolver)) {
            match field_result {
                Ok(json_val) => {
                    field_values.push(json_val);
                }
                Err(e) => {
                    tracing::warn!("Failed to decode composite field: {:?}", e);
                }
            }
        }

        // Create an object if we have named fields, otherwise an array
        let json_value = if has_named_fields && field_names.len() == field_values.len() {
            let obj: serde_json::Map<String, JsonValue> = field_names
                .into_iter()
                .zip(field_values)
                .filter_map(|(name, val)| name.map(|n| (n, val)))
                .collect();
            JsonValue::Object(obj)
        } else {
            JsonValue::Array(field_values)
        };

        Ok((type_name, json_value))
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Get type name from path
        let type_name = value.path().last().map(|s| s.to_string());
        let variant_name = value.name();

        // Special handling for Option::None - return null
        if variant_name == "None" {
            // Consume fields
            let fields_composite = value.fields();
            while let Some(field_result) =
                fields_composite.decode_item(ValueExtractor::new(self.resolver))
            {
                let _ = field_result;
            }
            return Ok((type_name, JsonValue::Null));
        }

        // Special handling for Option::Some - unwrap and return just the inner value
        if variant_name == "Some" {
            let fields_composite = value.fields();
            if let Some(field_result) =
                fields_composite.decode_item(ValueExtractor::new(self.resolver))
            {
                match field_result {
                    Ok(inner_value) => return Ok((type_name, inner_value)),
                    Err(_) => return Ok((type_name, JsonValue::Null)),
                }
            }
            return Ok((type_name, JsonValue::Null));
        }

        // Special handling for MultiAddress::Id variant - extract AccountId32 as hex
        if type_name.as_deref() == Some("MultiAddress") && variant_name == "Id" {
            let bytes = value.bytes_from_start();
            // Skip the variant index byte, then read 32 bytes for AccountId32
            if bytes.len() >= 33 {
                let hex_string = format!("0x{}", hex::encode(&bytes[1..33]));
                return Ok((type_name, JsonValue::String(hex_string)));
            }
        }

        // Check if this is a basic enum (all variants have no data)
        let is_basic = is_basic_enum(self.resolver, type_id);

        // For basic enums, return just the variant name as a string
        if is_basic {
            // Consume fields (there shouldn't be any)
            let fields_composite = value.fields();
            while let Some(field_result) =
                fields_composite.decode_item(ValueExtractor::new(self.resolver))
            {
                let _ = field_result;
            }
            return Ok((type_name, JsonValue::String(variant_name.to_string())));
        }

        // Non-basic enum - decode fields and wrap in object
        let mut fields = Vec::new();
        let fields_composite = value.fields();
        while let Some(field_result) =
            fields_composite.decode_item(ValueExtractor::new(self.resolver))
        {
            match field_result {
                Ok(json_val) => fields.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode variant field: {:?}", e);
                }
            }
        }

        // Convert variant name to lowerCamelCase for the key
        let key = lowercase_first_char(variant_name);

        // Create variant JSON representation as {"variantName": value}
        let json_value = if fields.is_empty() {
            serde_json::json!({ key: JsonValue::Null })
        } else if fields.len() == 1 {
            serde_json::json!({ key: fields[0].clone() })
        } else {
            serde_json::json!({ key: fields })
        };

        Ok((type_name, json_value))
    }

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Decode all sequence items
        let mut items = Vec::new();
        while let Some(item_result) = value.decode_item(ValueExtractor::new(self.resolver)) {
            match item_result {
                Ok(json_val) => items.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode sequence item: {:?}", e);
                }
            }
        }

        Ok((None, JsonValue::Array(items)))
    }

    fn visit_array<'scale, 'resolver>(
        self,
        value: &mut scale_decode::visitor::types::Array<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Decode all array items
        let mut items = Vec::new();
        while let Some(item_result) = value.decode_item(ValueExtractor::new(self.resolver)) {
            match item_result {
                Ok(json_val) => items.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode array item: {:?}", e);
                }
            }
        }

        // Check if this is a byte array (all items are u8 numbers)
        // Convert to hex string if so
        if items.len() >= 2 {
            let mut is_byte_array = true;
            let mut bytes = Vec::with_capacity(items.len());
            for item in &items {
                if let JsonValue::Number(n) = item
                    && let Some(byte) = n.as_u64()
                    && byte <= 255
                {
                    bytes.push(byte as u8);
                    continue;
                }
                is_byte_array = false;
                break;
            }
            if is_byte_array && bytes.len() == items.len() {
                return Ok((
                    None,
                    JsonValue::String(format!("0x{}", hex::encode(&bytes))),
                ));
            }
        }

        Ok((None, JsonValue::Array(items)))
    }

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok((None, serde_json::json!(value)))
    }

    fn visit_u16<'scale, 'resolver>(
        self,
        value: u16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok((None, serde_json::json!(value)))
    }

    fn visit_u32<'scale, 'resolver>(
        self,
        value: u32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok((None, serde_json::json!(value)))
    }

    fn visit_u64<'scale, 'resolver>(
        self,
        value: u64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok((None, serde_json::json!(value)))
    }

    fn visit_u128<'scale, 'resolver>(
        self,
        value: u128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok((None, serde_json::json!(value.to_string())))
    }

    fn visit_bool<'scale, 'resolver>(
        self,
        value: bool,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok((None, serde_json::json!(value)))
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok((None, JsonValue::Null))
    }
}

/// Visitor that extracts just the JSON value without type information
/// Used for recursive decoding of composite/variant/sequence fields
struct ValueExtractor<'r, R> {
    resolver: &'r R,
}

impl<'r, R> ValueExtractor<'r, R> {
    fn new(resolver: &'r R) -> Self {
        Self { resolver }
    }
}

impl<'r, R> Visitor for ValueExtractor<'r, R>
where
    R: TypeResolver,
    R::TypeId: Clone,
{
    type Value<'scale, 'resolver> = JsonValue;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Collect field names before decoding (since decode_item consumes them)
        let field_names: Vec<Option<String>> = value
            .fields()
            .iter()
            .map(|f| f.name.map(|s| s.to_lower_camel_case()))
            .collect();
        let has_named_fields = field_names.iter().any(|n| n.is_some());

        let mut field_values = Vec::new();
        while let Some(field_result) = value.decode_item(ValueExtractor::new(self.resolver)) {
            match field_result {
                Ok(json_val) => field_values.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode composite field: {:?}", e);
                }
            }
        }

        if has_named_fields && field_names.len() == field_values.len() {
            let obj: serde_json::Map<String, JsonValue> = field_names
                .into_iter()
                .zip(field_values)
                .filter_map(|(name, val)| name.map(|n| (n, val)))
                .collect();
            Ok(JsonValue::Object(obj))
        } else {
            Ok(JsonValue::Array(field_values))
        }
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let variant_name = value.name();

        // Special handling for Option::None - return null
        if variant_name == "None" {
            // Consume fields
            let fields_composite = value.fields();
            while let Some(field_result) =
                fields_composite.decode_item(ValueExtractor::new(self.resolver))
            {
                let _ = field_result;
            }
            return Ok(JsonValue::Null);
        }

        // Special handling for Option::Some - unwrap and return just the inner value
        if variant_name == "Some" {
            let fields_composite = value.fields();
            if let Some(field_result) =
                fields_composite.decode_item(ValueExtractor::new(self.resolver))
            {
                match field_result {
                    Ok(inner_value) => return Ok(inner_value),
                    Err(_) => return Ok(JsonValue::Null),
                }
            }
            return Ok(JsonValue::Null);
        }

        // Check if this is a basic enum (all variants have no data)
        let is_basic = is_basic_enum(self.resolver, type_id);

        // For basic enums, return just the variant name as a string
        if is_basic {
            // Consume fields (there shouldn't be any)
            let fields_composite = value.fields();
            while let Some(field_result) =
                fields_composite.decode_item(ValueExtractor::new(self.resolver))
            {
                let _ = field_result;
            }
            return Ok(JsonValue::String(variant_name.to_string()));
        }

        // Non-basic enum - decode fields and wrap in object
        let mut fields = Vec::new();
        let fields_composite = value.fields();
        while let Some(field_result) =
            fields_composite.decode_item(ValueExtractor::new(self.resolver))
        {
            match field_result {
                Ok(json_val) => fields.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode variant field: {:?}", e);
                }
            }
        }

        // Convert variant name to lowerCamelCase for the key
        let key = lowercase_first_char(variant_name);

        // Create variant JSON representation as {"variantName": value}
        if fields.is_empty() {
            Ok(serde_json::json!({ key: JsonValue::Null }))
        } else if fields.len() == 1 {
            Ok(serde_json::json!({ key: fields[0].clone() }))
        } else {
            Ok(serde_json::json!({ key: fields }))
        }
    }

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item_result) = value.decode_item(ValueExtractor::new(self.resolver)) {
            match item_result {
                Ok(json_val) => items.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode sequence item: {:?}", e);
                }
            }
        }
        Ok(JsonValue::Array(items))
    }

    fn visit_array<'scale, 'resolver>(
        self,
        value: &mut scale_decode::visitor::types::Array<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Decode all array items
        let mut items = Vec::new();
        while let Some(item_result) = value.decode_item(ValueExtractor::new(self.resolver)) {
            match item_result {
                Ok(json_val) => items.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode array item: {:?}", e);
                }
            }
        }

        // Check if this is a byte array (all items are u8 numbers)
        // Convert to hex string if so
        if items.len() >= 2 {
            let mut is_byte_array = true;
            let mut bytes = Vec::with_capacity(items.len());
            for item in &items {
                if let JsonValue::Number(n) = item
                    && let Some(byte) = n.as_u64()
                    && byte <= 255
                {
                    bytes.push(byte as u8);
                    continue;
                }
                is_byte_array = false;
                break;
            }
            if is_byte_array && bytes.len() == items.len() {
                return Ok(JsonValue::String(format!("0x{}", hex::encode(&bytes))));
            }
        }

        Ok(JsonValue::Array(items))
    }

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u16<'scale, 'resolver>(
        self,
        value: u16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u32<'scale, 'resolver>(
        self,
        value: u32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u64<'scale, 'resolver>(
        self,
        value: u64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_u128<'scale, 'resolver>(
        self,
        value: u128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(serde_json::json!(value.to_string()))
    }

    fn visit_bool<'scale, 'resolver>(
        self,
        value: bool,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(serde_json::json!(value))
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(JsonValue::Null)
    }
}

// ================================================================================================
// Post-Processing Functions
// ================================================================================================

/// Convert JSON value, replacing byte arrays with hex strings and all numbers with strings recursively
///
/// This matches substrate-api-sidecar's behavior of returning all numeric values as strings
/// for consistency across the API.
pub fn convert_bytes_to_hex(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Number(n) => {
            // Convert all numbers to strings to match substrate-api-sidecar behavior
            JsonValue::String(n.to_string())
        }
        JsonValue::Array(arr) => {
            // Check if this is a byte array (non-empty and all elements are numbers 0-255)
            // We must check !arr.is_empty() to avoid converting empty arrays to "0x"
            let is_byte_array = !arr.is_empty()
                && arr.iter().all(|v| match v {
                    JsonValue::Number(n) => n.as_u64().is_some_and(|val| val <= 255),
                    _ => false,
                });

            if is_byte_array {
                // Convert to hex string
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                JsonValue::String(format!("0x{}", hex::encode(&bytes)))
            } else {
                // Recurse into array elements
                let converted: Vec<JsonValue> = arr.into_iter().map(convert_bytes_to_hex).collect();

                // If array has single element, unwrap it (this handles cases like ["0x..."] -> "0x...")
                // This is specific to how the data is formatted in substrate-api-sidecar
                match converted.len() {
                    1 => match converted.into_iter().next() {
                        Some(v) => v,
                        None => JsonValue::Array(vec![]),
                    },
                    _ => JsonValue::Array(converted),
                }
            }
        }
        JsonValue::Object(mut map) => {
            // Check if this is a bitvec object (scale-value represents bitvecs specially)
            if let Some(JsonValue::Array(bits)) = map.get("__bitvec__values__") {
                // Convert boolean array to bytes, then to hex
                // BitVec uses LSB0 ordering (least significant bit first within each byte)
                let mut bytes = Vec::new();
                let mut current_byte = 0u8;

                for (i, bit) in bits.iter().enumerate() {
                    if let Some(true) = bit.as_bool() {
                        current_byte |= 1 << (i % 8);
                    }

                    // Every 8 bits, push the byte and reset
                    if (i + 1) % 8 == 0 {
                        bytes.push(current_byte);
                        current_byte = 0;
                    }
                }

                // Push any remaining bits
                if bits.len() % 8 != 0 {
                    bytes.push(current_byte);
                }

                return JsonValue::String(format!("0x{}", hex::encode(&bytes)));
            }

            // Recurse into object values
            for (_, v) in map.iter_mut() {
                *v = convert_bytes_to_hex(v.clone());
            }
            JsonValue::Object(map)
        }
        other => other,
    }
}

/// Unified transformation function that combines byte-to-hex conversion and structural transformations
/// in a single pass through the JSON tree.
///
/// This performs all of the following transformations in one traversal:
/// - Converts byte arrays to hex strings
/// - Converts numbers to strings
/// - Handles bitvec special encoding
/// - Transforms snake_case keys to camelCase
/// - Optionally decodes AccountId32 to SS58 format
/// - Unwraps single-element arrays
///
/// Note: Enum serialization (basic vs non-basic) should be handled at the type level
/// in the visitor chain using `is_basic_enum()`.
pub fn transform_json_unified(value: JsonValue, ss58_prefix: Option<u16>) -> JsonValue {
    match value {
        JsonValue::Number(n) => {
            // Convert all numbers to strings to match substrate-api-sidecar behavior
            JsonValue::String(n.to_string())
        }
        JsonValue::Array(arr) => {
            // Check if this is a byte array (all elements are numbers 0-255)
            // Require at least 2 elements - single-element arrays are typically newtype wrappers
            // (e.g., ValidatorIndex(32) -> [32]), not actual byte data
            let is_byte_array = arr.len() > 1
                && arr.iter().all(|v| match v {
                    JsonValue::Number(n) => n.as_u64().is_some_and(|val| val <= 255),
                    _ => false,
                });

            if is_byte_array {
                // Convert to hex string
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                JsonValue::String(format!("0x{}", hex::encode(&bytes)))
            } else {
                // Recurse into array elements
                let converted: Vec<JsonValue> = arr
                    .into_iter()
                    .map(|v| transform_json_unified(v, ss58_prefix))
                    .collect();

                // If array has single element, unwrap it
                match converted.len() {
                    1 => match converted.into_iter().next() {
                        Some(v) => v,
                        None => JsonValue::Array(vec![]),
                    },
                    _ => JsonValue::Array(converted),
                }
            }
        }
        JsonValue::Object(map) => {
            // Check if this is a bitvec object (scale-value represents bitvecs specially)
            if let Some(JsonValue::Array(bits)) = map.get("__bitvec__values__") {
                // Convert boolean array to bytes, then to hex
                let mut bytes = Vec::new();
                let mut current_byte = 0u8;

                for (i, bit) in bits.iter().enumerate() {
                    if let Some(true) = bit.as_bool() {
                        current_byte |= 1 << (i % 8);
                    }

                    if (i + 1) % 8 == 0 {
                        bytes.push(current_byte);
                        current_byte = 0;
                    }
                }

                if bits.len() % 8 != 0 {
                    bytes.push(current_byte);
                }

                return JsonValue::String(format!("0x{}", hex::encode(&bytes)));
            }

            // Regular object: transform keys from snake_case to camelCase and recurse
            // Note: Heuristic for SCALE enum variants removed - enum serialization should be
            // handled at the type level in the visitor chain using is_basic_enum()
            let transformed: serde_json::Map<String, JsonValue> = map
                .into_iter()
                .map(|(key, val)| {
                    let camel_key = key.to_lower_camel_case();
                    (camel_key, transform_json_unified(val, ss58_prefix))
                })
                .collect();
            JsonValue::Object(transformed)
        }
        JsonValue::String(s) => {
            // Try to decode as SS58 address if ss58_prefix is provided
            if let Some(prefix) = ss58_prefix
                && s.starts_with("0x")
                && (s.len() == 66 || s.len() == 68)
                && let Some(ss58_addr) = crate::utils::decode_address_to_ss58(&s, prefix)
            {
                return JsonValue::String(ss58_addr);
            }

            JsonValue::String(s)
        }
        other => other,
    }
}

/// Convert AccountId32 (as hex or array) to SS58 format
pub fn try_convert_accountid_to_ss58(value: &JsonValue, ss58_prefix: u16) -> Option<JsonValue> {
    if let Some(hex_str) = value.as_str()
        && hex_str.starts_with("0x")
        && hex_str.len() == 66
    {
        match hex::decode(&hex_str[2..]) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                let account_id = AccountId32::from(arr);
                let ss58 = account_id.to_ss58check_with_version(ss58_prefix.into());
                return Some(JsonValue::String(ss58));
            }
            _ => {}
        }
    }

    if let Some(arr) = value.as_array()
        && arr.len() == 32
    {
        let mut bytes = [0u8; 32];
        for (i, val) in arr.iter().enumerate() {
            if let Some(byte) = val.as_u64() {
                bytes[i] = byte as u8;
            } else {
                return None;
            }
        }
        let account_id = AccountId32::from(bytes);
        let ss58 = account_id.to_ss58check_with_version(ss58_prefix.into());
        return Some(JsonValue::String(ss58));
    }

    None
}
