//! Event decoding and transformation for block data.
//!
//! This module provides:
//! - `EventsVisitor` for extracting event information from System.Events storage
//! - Post-processing functions for transforming decoded event data to JSON

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
pub struct EventsVisitor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> Default for EventsVisitor<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> EventsVisitor<R> {
    pub fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for EventsVisitor<R>
where
    R: TypeResolver,
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
        while let Some(event_record_result) = value.decode_item(EventRecordVisitor::new()) {
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
struct EventRecordVisitor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> EventRecordVisitor<R> {
    fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for EventRecordVisitor<R>
where
    R: TypeResolver,
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
        if let Some(event_result) = value.decode_item(PalletEventVisitor::new(phase)) {
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
struct PalletEventVisitor<R> {
    phase: EventPhase,
    _marker: core::marker::PhantomData<R>,
}

impl<R> PalletEventVisitor<R> {
    fn new(phase: EventPhase) -> Self {
        Self {
            phase,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for PalletEventVisitor<R>
where
    R: TypeResolver,
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
        if let Some(inner_event_result) =
            fields_composite.decode_item(ActualEventVisitor::new(self.phase, pallet_name))
        {
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
struct ActualEventVisitor<R> {
    phase: EventPhase,
    pallet_name: String,
    _marker: core::marker::PhantomData<R>,
}

impl<R> ActualEventVisitor<R> {
    fn new(phase: EventPhase, pallet_name: String) -> Self {
        Self {
            phase,
            pallet_name,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for ActualEventVisitor<R>
where
    R: TypeResolver,
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
        while let Some(field_result) = fields_composite.decode_item(FieldWithTypeExtractor::new()) {
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
struct FieldWithTypeExtractor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> FieldWithTypeExtractor<R> {
    fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for FieldWithTypeExtractor<R>
where
    R: TypeResolver,
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

        // Decode all fields recursively into a JSON object
        let mut fields = Vec::new();
        while let Some(field_result) = value.decode_item(ValueExtractor::new()) {
            match field_result {
                Ok(json_val) => fields.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode composite field: {:?}", e);
                }
            }
        }

        // If named fields, create an object; otherwise create an array
        let json_value = if value.has_unnamed_fields() {
            JsonValue::Array(fields)
        } else {
            // For named fields, we'd need field names which aren't easily accessible
            // For now, just use an array
            JsonValue::Array(fields)
        };

        Ok((type_name, json_value))
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Get type name from path
        let type_name = value.path().last().map(|s| s.to_string());
        let variant_name = value.name();

        // Special handling for MultiAddress::Id variant - extract AccountId32 as hex
        if type_name.as_deref() == Some("MultiAddress") && variant_name == "Id" {
            let bytes = value.bytes_from_start();
            // Skip the variant index byte, then read 32 bytes for AccountId32
            if bytes.len() >= 33 {
                let hex_string = format!("0x{}", hex::encode(&bytes[1..33]));
                return Ok((type_name, JsonValue::String(hex_string)));
            }
        }

        // Decode variant fields
        let mut fields = Vec::new();
        let fields_composite = value.fields();
        while let Some(field_result) = fields_composite.decode_item(ValueExtractor::new()) {
            match field_result {
                Ok(json_val) => fields.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode variant field: {:?}", e);
                }
            }
        }

        // Create variant JSON representation
        let json_value = if fields.is_empty() {
            JsonValue::String(variant_name.to_string())
        } else if fields.len() == 1 {
            serde_json::json!({
                "name": variant_name,
                "value": fields[0].clone()
            })
        } else {
            serde_json::json!({
                "name": variant_name,
                "values": fields
            })
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
        while let Some(item_result) = value.decode_item(ValueExtractor::new()) {
            match item_result {
                Ok(json_val) => items.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode sequence item: {:?}", e);
                }
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
struct ValueExtractor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> ValueExtractor<R> {
    fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> Visitor for ValueExtractor<R>
where
    R: TypeResolver,
{
    type Value<'scale, 'resolver> = JsonValue;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut fields = Vec::new();
        while let Some(field_result) = value.decode_item(ValueExtractor::new()) {
            match field_result {
                Ok(json_val) => fields.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode composite field: {:?}", e);
                }
            }
        }
        Ok(JsonValue::Array(fields))
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let variant_name = value.name();
        let mut fields = Vec::new();
        let fields_composite = value.fields();
        while let Some(field_result) = fields_composite.decode_item(ValueExtractor::new()) {
            match field_result {
                Ok(json_val) => fields.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode variant field: {:?}", e);
                }
            }
        }

        if fields.is_empty() {
            Ok(JsonValue::String(variant_name.to_string()))
        } else if fields.len() == 1 {
            Ok(serde_json::json!({
                "name": variant_name,
                "value": fields[0].clone()
            }))
        } else {
            Ok(serde_json::json!({
                "name": variant_name,
                "values": fields
            }))
        }
    }

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item_result) = value.decode_item(ValueExtractor::new()) {
            match item_result {
                Ok(json_val) => items.push(json_val),
                Err(e) => {
                    tracing::warn!("Failed to decode sequence item: {:?}", e);
                }
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
/// - Simplifies SCALE enum variants
/// - Optionally decodes AccountId32 to SS58 format
/// - Unwraps single-element arrays
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

            // Check if this is a SCALE enum variant: {"name": "X", "values": Y}
            if map.len() == 2
                && let (Some(JsonValue::String(name)), Some(values)) =
                    (map.get("name"), map.get("values"))
            {
                // If values is "0x" (empty string) or [] (empty array), return just the name as string
                // This is evident in class, and paysFee
                let is_empty = match values {
                    JsonValue::String(v) => v == "0x",
                    JsonValue::Array(arr) => arr.is_empty(),
                    _ => false,
                };

                if is_empty {
                    // Special case: "None" variant should serialize as JSON null
                    if name == "None" {
                        return JsonValue::Null;
                    }
                    return JsonValue::String(name.clone());
                }

                // For args (when ss58_prefix is Some), transform to {"<name>": <transformed_values>}
                if ss58_prefix.is_some() {
                    // Only lowercase the first letter for CamelCase names (e.g., "PreRuntime" -> "preRuntime")
                    // Keep snake_case names as-is (e.g., "inbound_messages_data" stays unchanged)
                    let key = crate::utils::lowercase_first_char(name);
                    let transformed_value = transform_json_unified(values.clone(), ss58_prefix);

                    let mut result = serde_json::Map::new();
                    result.insert(key, transformed_value);
                    return JsonValue::Object(result);
                }
                // For events (when ss58_prefix is None), we don't transform the enum further
                // Fall through to regular object handling
            }

            // Regular object: transform keys from snake_case to camelCase and recurse
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
