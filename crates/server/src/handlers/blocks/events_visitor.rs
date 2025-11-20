/// Custom visitor for extracting event information with type names from System.Events storage
use scale_decode::{
    visitor::{types::{Composite, Sequence, Variant}, TypeIdFor, Unexpected},
    Visitor,
};
use scale_type_resolver::TypeResolver;
use serde_json::Value as JsonValue;

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
        let pallet_name = value.name().to_lowercase();

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
        while let Some(field_result) = fields_composite.decode_item(FieldWithTypeExtractor::new())
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
