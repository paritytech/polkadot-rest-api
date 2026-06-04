// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Event decoding for block data.
//!
//! This module provides `EventsVisitor` for extracting event information from
//! System.Events storage. Event fields are decoded using `EventJsonVisitor`
//! (from `args.rs`) which handles all type-aware transformations at decode time:
//! AccountId32 → SS58, numbers → strings, camelCase keys, byte arrays → hex.

use scale_decode::{
    Visitor,
    visitor::{
        TypeIdFor, Unexpected,
        types::{Composite, Sequence, Variant},
    },
};
use scale_info::PortableRegistry;
use serde_json::Value as JsonValue;

use super::args::EventJsonVisitor;

/// Represents a single event with its metadata and decoded fields
#[derive(Debug, Clone)]
pub struct EventInfo {
    pub phase: EventPhase,
    pub pallet_name: String,
    pub event_name: String,
    pub fields: Vec<JsonValue>,
}

/// Event phase extracted from EventRecord
#[derive(Debug, Clone)]
pub enum EventPhase {
    Initialization,
    ApplyExtrinsic(u32),
    Finalization,
}

// ================================================================================================
// Event Visitor Types
// ================================================================================================

/// Visitor that collects all events with their decoded field data.
/// Uses `EventJsonVisitor` for type-aware field decoding.
pub struct EventsVisitor<'r> {
    ss58_prefix: u16,
    resolver: &'r PortableRegistry,
}

impl<'r> EventsVisitor<'r> {
    pub fn new(ss58_prefix: u16, resolver: &'r PortableRegistry) -> Self {
        Self {
            ss58_prefix,
            resolver,
        }
    }
}

impl<'r> Visitor for EventsVisitor<'r> {
    type Value<'scale, 'resolver> = Vec<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut events = Vec::new();

        while let Some(event_record_result) =
            value.decode_item(EventRecordVisitor::new(self.ss58_prefix, self.resolver))
        {
            match event_record_result {
                Ok(Some(event_info)) => events.push(event_info),
                Ok(None) => {
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

/// Visitor for a single EventRecord
struct EventRecordVisitor<'r> {
    ss58_prefix: u16,
    resolver: &'r PortableRegistry,
}

impl<'r> EventRecordVisitor<'r> {
    fn new(ss58_prefix: u16, resolver: &'r PortableRegistry) -> Self {
        Self {
            ss58_prefix,
            resolver,
        }
    }
}

impl<'r> Visitor for EventRecordVisitor<'r> {
    type Value<'scale, 'resolver> = Option<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let phase = if let Some(phase_result) = value.decode_item(PhaseExtractor::new()) {
            phase_result?
        } else {
            EventPhase::Finalization
        };

        if let Some(event_result) = value.decode_item(PalletEventVisitor::new(
            phase,
            self.ss58_prefix,
            self.resolver,
        )) {
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
struct PhaseExtractor;

impl PhaseExtractor {
    fn new() -> Self {
        Self
    }
}

impl Visitor for PhaseExtractor {
    type Value<'scale, 'resolver> = EventPhase;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let variant_name = value.name();
        let fields = value.fields();

        match variant_name {
            "ApplyExtrinsic" => {
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
struct U32Extractor;

impl U32Extractor {
    fn new() -> Self {
        Self
    }
}

impl Visitor for U32Extractor {
    type Value<'scale, 'resolver> = u32;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

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

/// Visitor for the pallet-level variant
struct PalletEventVisitor<'r> {
    phase: EventPhase,
    ss58_prefix: u16,
    resolver: &'r PortableRegistry,
}

impl<'r> PalletEventVisitor<'r> {
    fn new(phase: EventPhase, ss58_prefix: u16, resolver: &'r PortableRegistry) -> Self {
        Self {
            phase,
            ss58_prefix,
            resolver,
        }
    }
}

impl<'r> Visitor for PalletEventVisitor<'r> {
    type Value<'scale, 'resolver> = Option<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let pallet_name = crate::utils::lowercase_first_char(value.name());
        let fields_composite = value.fields();

        if let Some(inner_event_result) = fields_composite.decode_item(ActualEventVisitor::new(
            self.phase,
            pallet_name,
            self.ss58_prefix,
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

/// Visitor for the actual event variant.
/// Decodes event fields using `EventJsonVisitor` for type-aware JSON serialization.
struct ActualEventVisitor<'r> {
    phase: EventPhase,
    pallet_name: String,
    ss58_prefix: u16,
    resolver: &'r PortableRegistry,
}

impl<'r> ActualEventVisitor<'r> {
    fn new(
        phase: EventPhase,
        pallet_name: String,
        ss58_prefix: u16,
        resolver: &'r PortableRegistry,
    ) -> Self {
        Self {
            phase,
            pallet_name,
            ss58_prefix,
            resolver,
        }
    }
}

impl<'r> Visitor for ActualEventVisitor<'r> {
    type Value<'scale, 'resolver> = Option<EventInfo>;
    type Error = scale_decode::Error;
    type TypeResolver = PortableRegistry;

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let event_name = value.name().to_string();
        let mut event_fields = Vec::new();

        let fields_composite = value.fields();

        while let Some(field_result) =
            fields_composite.decode_item(EventJsonVisitor::new(self.ss58_prefix, self.resolver))
        {
            match field_result {
                Ok(json_value) => {
                    event_fields.push(json_value);
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
