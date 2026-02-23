// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for /coretime/overview endpoint.
//!
//! Returns a comprehensive overview of all cores, supporting both relay chains
//! and coretime chains with different response structures.
//!
//! ## Relay Chain Response
//! When connected to a relay chain (Polkadot, Kusama, etc.), returns data from:
//! - CoretimeAssignmentProvider::CoreDescriptors - core assignment state
//! - CoretimeAssignmentProvider::CoreSchedules - scheduled core assignments
//! - Paras::ParaLifecycles - parachain lifecycle states
//!
//! ## Coretime Chain Response
//! When connected to a coretime chain, returns data from:
//! - Broker::Workload - current core assignments
//! - Broker::Workplan - future planned work
//! - Broker::Leases - lease information
//! - Broker::Reservations - reserved cores
//! - Broker::Regions - purchased regions

use crate::extractors::JsonQuery;
use crate::handlers::coretime::common::{
    AtResponse, CORE_TYPE_BULK, CORE_TYPE_LEASE, CORE_TYPE_ONDEMAND, CORE_TYPE_RESERVATION,
    CoreAssignment, CoretimeError, CoretimeQueryParams, ScheduleItem, TASK_POOL, has_broker_pallet,
    has_coretime_assignment_provider_pallet, has_paras_pallet,
};
use crate::handlers::coretime::leases::fetch_leases;
use crate::handlers::coretime::regions::{RegionInfo, fetch_regions};
use crate::handlers::coretime::reservations::{ReservationInfo, fetch_reservations};
use crate::state::AppState;
use crate::utils::{BlockId, resolve_block};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use parity_scale_codec::Decode;
use polkadot_rest_api_config::ChainType;
use primitive_types::H256;
use scale_decode::DecodeAsType;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Response Types
// ============================================================================

/// Workload info for a single core.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadInfo {
    /// Whether this core is assigned to the instantaneous pool.
    pub is_pool: bool,
    /// Whether this core is assigned to a specific task.
    pub is_task: bool,
    /// The CoreMask as a hex string.
    pub mask: String,
    /// The task assignment: task ID as string, "Pool", or empty for idle.
    pub task: String,
}

/// Workplan entry for a single timeslice.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkplanEntry {
    /// The core index.
    pub core: u32,
    /// The timeslice this workplan is for.
    pub timeslice: u32,
    /// The scheduled work items.
    pub info: Vec<WorkloadInfo>,
}

/// Core type classification with optional details.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoreType {
    /// The condition: "lease", "bulk", "reservation", or "ondemand".
    pub condition: String,
    /// Optional details depending on condition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<CoreTypeDetails>,
}

/// Details for core type (varies by condition).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoreTypeDetails {
    /// For reservation: the mask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    /// For lease: the until timeslice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<u32>,
}

/// Information about a single core.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoreInfo {
    /// The core index.
    pub core_id: u32,
    /// The para/task ID assigned to this core.
    pub para_id: String,
    /// Current workload information.
    pub workload: WorkloadInfo,
    /// Planned future work for this core.
    pub workplan: Vec<WorkplanEntry>,
    /// The type of assignment (lease/bulk/reservation/ondemand).
    #[serde(rename = "type")]
    pub core_type: CoreType,
    /// Regions associated with this core.
    pub regions: Vec<RegionInfo>,
}

/// Response for GET /coretime/overview endpoint (coretime chain).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeOverviewResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// List of cores with their complete information.
    pub cores: Vec<CoreInfo>,
}

// ============================================================================
// Relay Chain Response Types
// ============================================================================

/// Assignment state for a core on a relay chain.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayAssignmentInfo {
    /// The task assignment: task ID as string, "Pool", or "Idle".
    pub task: String,
    /// The ratio of the assignment (parts per 57600).
    pub ratio: u32,
    /// Remaining parts to be assigned.
    pub remaining: u32,
    /// Whether this is a task assignment.
    pub is_task: bool,
    /// Whether this is a pool assignment.
    pub is_pool: bool,
}

/// Current work state for a core on a relay chain.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayCurrentWork {
    /// List of assignments for this core.
    pub assignments: Vec<RelayAssignmentInfo>,
    /// Hint for when this work ends (block number), if any.
    pub end_hint: Option<String>,
    /// Current position in the schedule.
    pub pos: u32,
    /// Step size for advancing through the schedule.
    pub step: u32,
}

/// Queue descriptor for a core on a relay chain.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayQueueDescriptor {
    /// First item in the queue.
    pub first: String,
    /// Last item in the queue.
    pub last: String,
}

/// Core descriptor info for a relay chain.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayCoreDescriptorInfo {
    /// Current work state.
    pub current_work: RelayCurrentWork,
    /// Queue state.
    pub queue: RelayQueueDescriptor,
}

/// Core descriptor for a relay chain (includes core index and parachain lifecycle).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayCoreDescriptor {
    /// The core index.
    pub core: u32,
    /// The parachain ID assigned to this core (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub para_id: Option<u32>,
    /// The parachain lifecycle type (e.g., "Parachain", "Parathread", "Onboarding").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<String>,
    /// Core descriptor info.
    pub info: RelayCoreDescriptorInfo,
}

/// Response for GET /coretime/overview endpoint (relay chain).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayOverviewResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// List of cores with their descriptors and parachain info.
    pub cores: Vec<RelayCoreDescriptor>,
    /// Core schedules (raw data from storage).
    pub core_schedules: Vec<serde_json::Value>,
}

// ============================================================================
// Internal Types for Relay Chain
// ============================================================================

/// Internal type for decoding CoreAssignment from relay chain.
/// Matches pallet_broker::CoreAssignment.
#[derive(Debug, Clone, Decode, DecodeAsType)]
enum RelayCoreAssignment {
    Idle,
    Pool,
    Task(u32),
}

/// Internal type for decoding AssignmentState from relay chain.
/// Note: ratio and remaining are PartsOf57600(u16) on-chain, DecodeAsType handles newtype unwrapping.
#[derive(Debug, Clone, Decode, DecodeAsType)]
struct RelayAssignmentState {
    ratio: u16,
    remaining: u16,
}

/// Internal type for decoding WorkState from relay chain.
/// Note: assignments is Vec<(CoreAssignment, AssignmentState)> - a Vec of TUPLES.
/// step is PartsOf57600(u16) on-chain.
#[derive(Debug, Clone, Decode, DecodeAsType)]
struct RelayWorkState {
    // Assignments as tuples (CoreAssignment, AssignmentState)
    assignments: Vec<(RelayCoreAssignment, RelayAssignmentState)>,
    end_hint: Option<u32>,
    pos: u16,
    step: u16,
}

/// Internal type for decoding QueueDescriptor from relay chain.
#[derive(Debug, Clone, Decode, DecodeAsType)]
struct RelayQueueState {
    first: u32,
    last: u32,
}

/// Internal type for decoding CoreDescriptor from relay chain.
#[derive(Debug, Clone, Decode, DecodeAsType)]
struct RelayCoreDescriptorRaw {
    queue: Option<RelayQueueState>,
    current_work: Option<RelayWorkState>,
}

/// On-chain ParaLifecycle enum for DecodeAsType decoding.
/// Matches polkadot_runtime_parachains::paras::ParaLifecycle.
#[derive(Debug, Clone, DecodeAsType)]
enum ParaLifecycleType {
    Onboarding,
    Parathread,
    Parachain,
    UpgradingParathread,
    DowngradingParachain,
    OffboardingParathread,
    OffboardingParachain,
}

impl ParaLifecycleType {
    fn as_str(&self) -> &'static str {
        match self {
            ParaLifecycleType::Onboarding => "Onboarding",
            ParaLifecycleType::Parathread => "Parathread",
            ParaLifecycleType::Parachain => "Parachain",
            ParaLifecycleType::UpgradingParathread => "UpgradingParathread",
            ParaLifecycleType::DowngradingParachain => "DowngradingParachain",
            ParaLifecycleType::OffboardingParathread => "OffboardingParathread",
            ParaLifecycleType::OffboardingParachain => "OffboardingParachain",
        }
    }
}

/// Parachain lifecycle from relay chain.
#[derive(Debug, Clone)]
struct ParaLifecycle {
    para_id: u32,
    lifecycle_type: Option<String>,
}

// ============================================================================
// Internal Types
// ============================================================================

/// Workload with full schedule item data.
#[derive(Debug, Clone)]
struct WorkloadWithSchedule {
    core: u32,
    items: Vec<ScheduleItem>,
}

/// Workplan entry with timeslice and schedule items.
#[derive(Debug, Clone)]
struct WorkplanWithSchedule {
    core: u32,
    timeslice: u32,
    items: Vec<ScheduleItem>,
}

// ============================================================================
// Main Handler
// ============================================================================

/// Handler for GET /coretime/overview endpoint.
///
/// Returns a comprehensive overview of all cores. The response structure differs
/// based on the chain type:
///
/// ## Relay Chain Response
/// - cores: List of core descriptors with assignments, queue state, and parachain lifecycle
/// - coreSchedules: Raw core schedule data
///
/// ## Coretime Chain Response
/// - cores: List of cores with workload, workplan, type, and regions
///
/// Query Parameters:
/// - at: Optional block number or hash to query at (defaults to latest finalized)
#[utoipa::path(
    get,
    path = "/v1/coretime/overview",
    tag = "coretime",
    summary = "Get coretime overview",
    description = "Returns an overview of all cores with assignments, queue state, workload, workplan, and regions.",
    params(
        ("at" = Option<String>, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Coretime overview", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn coretime_overview(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<CoretimeQueryParams>,
) -> Result<Response, CoretimeError> {
    // Parse the block ID if provided
    let block_id = match &params.at {
        None => None,
        Some(at_str) => Some(at_str.parse::<BlockId>()?),
    };

    // Resolve the block
    let resolved_block = resolve_block(&state, block_id).await?;

    // Get client at the resolved block hash
    let block_hash =
        H256::from_str(&resolved_block.hash).map_err(|_| CoretimeError::InvalidBlockHash)?;
    let client_at_block = state.client.at_block(block_hash).await?;

    let at = AtResponse {
        hash: resolved_block.hash.clone(),
        height: resolved_block.number.to_string(),
    };

    // Detect chain type and handle accordingly
    match state.chain_info.chain_type {
        ChainType::Relay => handle_relay_chain_overview(&client_at_block, at).await,
        ChainType::Coretime => {
            handle_coretime_chain_overview(&client_at_block, at, state.chain_info.ss58_prefix).await
        }
        _ => {
            // For other chain types, check which pallets are available
            if has_broker_pallet(&client_at_block) {
                handle_coretime_chain_overview(&client_at_block, at, state.chain_info.ss58_prefix)
                    .await
            } else if has_coretime_assignment_provider_pallet(&client_at_block) {
                handle_relay_chain_overview(&client_at_block, at).await
            } else {
                Err(CoretimeError::UnsupportedChainType)
            }
        }
    }
}

/// Handle overview request for relay chains.
async fn handle_relay_chain_overview(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    at: AtResponse,
) -> Result<Response, CoretimeError> {
    // Verify that required pallets exist
    if !has_coretime_assignment_provider_pallet(client_at_block) {
        return Err(CoretimeError::CoretimePalletNotFound);
    }

    // Fetch all data in parallel
    let (descriptors, schedules, lifecycles) = tokio::try_join!(
        fetch_core_descriptors(client_at_block),
        fetch_core_schedules(client_at_block),
        fetch_para_lifecycles(client_at_block)
    )?;

    // Build a map of para_id -> lifecycle for quick lookup
    let lifecycle_map: std::collections::HashMap<u32, String> = lifecycles
        .iter()
        .filter_map(|pl| {
            pl.lifecycle_type
                .as_ref()
                .map(|lt| (pl.para_id, lt.clone()))
        })
        .collect();

    // Combine descriptors with parachain info, matching sidecar's join semantics.
    //
    // Sidecar iterates paraLifecycles and for each parachain finds the FIRST matching
    // core descriptor (using .find()). This means:
    // - Only cores whose paraId exists in paraLifecycles are included
    // - Each paraId appears at most once (deduplicated)
    // - Pool cores are not included (they have no paraId match)
    //
    // We replicate this by: iterating core descriptors (preserving core-index order),
    // filtering by lifecycle match, and deduplicating by paraId.
    let mut seen_para_ids = std::collections::HashSet::new();
    let cores: Vec<RelayCoreDescriptor> = descriptors
        .into_iter()
        .map(|(core, raw)| {
            // Find the primary para_id from assignments
            // Assignments are tuples: (CoreAssignment, AssignmentState)
            let para_id = raw.current_work.as_ref().and_then(|cw| {
                cw.assignments
                    .iter()
                    .find_map(|(assignment, _state)| match assignment {
                        RelayCoreAssignment::Task(id) => Some(*id),
                        _ => None,
                    })
            });

            // Get lifecycle for this para
            let lifecycle = para_id.and_then(|pid| lifecycle_map.get(&pid).cloned());

            // Build the response structure
            // Assignments are tuples: (CoreAssignment, AssignmentState)
            let current_work = raw.current_work.map(|cw| RelayCurrentWork {
                assignments: cw
                    .assignments
                    .iter()
                    .map(|(assignment, state)| {
                        let (task, is_task, is_pool) = match assignment {
                            RelayCoreAssignment::Idle => (String::new(), false, false),
                            RelayCoreAssignment::Pool => (TASK_POOL.to_string(), false, true),
                            RelayCoreAssignment::Task(id) => (id.to_string(), true, false),
                        };
                        RelayAssignmentInfo {
                            task,
                            ratio: state.ratio as u32,
                            remaining: state.remaining as u32,
                            is_task,
                            is_pool,
                        }
                    })
                    .collect(),
                end_hint: cw.end_hint.map(|h| h.to_string()),
                pos: cw.pos as u32,
                step: cw.step as u32,
            });

            let queue = raw.queue.map(|q| RelayQueueDescriptor {
                first: q.first.to_string(),
                last: q.last.to_string(),
            });

            RelayCoreDescriptor {
                core,
                para_id,
                lifecycle,
                info: RelayCoreDescriptorInfo {
                    current_work: current_work.unwrap_or(RelayCurrentWork {
                        assignments: vec![],
                        end_hint: None,
                        pos: 0,
                        step: 0,
                    }),
                    queue: queue.unwrap_or(RelayQueueDescriptor {
                        first: "0".to_string(),
                        last: "0".to_string(),
                    }),
                },
            }
        })
        .filter(|core| {
            // Match sidecar's join: only include cores whose paraId has an
            // active lifecycle entry, and deduplicate by paraId (sidecar's
            // .find() returns only the first core per parachain).
            match core.para_id {
                Some(pid) if core.lifecycle.is_some() => seen_para_ids.insert(pid),
                _ => false,
            }
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(RelayOverviewResponse {
            at,
            cores,
            core_schedules: schedules,
        }),
    )
        .into_response())
}

/// Handle overview request for coretime chains.
async fn handle_coretime_chain_overview(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    at: AtResponse,
    ss58_prefix: u16,
) -> Result<Response, CoretimeError> {
    // Verify that the Broker pallet exists at this block
    if !has_broker_pallet(client_at_block) {
        return Err(CoretimeError::BrokerPalletNotFound);
    }

    // Fetch all data in parallel
    let (workloads, workplans, leases, reservations, regions) = tokio::try_join!(
        fetch_workloads_full(client_at_block),
        fetch_workplans(client_at_block),
        fetch_leases(client_at_block),
        fetch_reservations(client_at_block),
        fetch_regions(client_at_block, ss58_prefix)
    )?;

    // Build the system paras list (tasks in reservations)
    let system_paras: Vec<String> = reservations.iter().map(|r| r.task.clone()).collect();

    // Build the lease tasks map for quick lookup
    let lease_map: std::collections::HashMap<String, u32> = leases
        .iter()
        .map(|l| (l.task.to_string(), l.until))
        .collect();

    // Build the cores response
    let mut cores: Vec<CoreInfo> = workloads
        .into_iter()
        .map(|wl| {
            let workload_info = extract_workload_info(&wl.items);
            let para_id = workload_info.task.clone();

            // Determine core type
            let core_type = determine_core_type(&para_id, &system_paras, &reservations, &lease_map);

            // Filter workplan entries for this core
            let core_workplan: Vec<WorkplanEntry> = workplans
                .iter()
                .filter(|wp| wp.core == wl.core)
                .map(workplan_to_entry)
                .collect();

            // Filter regions for this core
            let core_regions: Vec<RegionInfo> = regions
                .iter()
                .filter(|r| r.core == wl.core)
                .cloned()
                .collect();

            CoreInfo {
                core_id: wl.core,
                para_id,
                workload: workload_info,
                workplan: core_workplan,
                core_type,
                regions: core_regions,
            }
        })
        .collect();

    // Sort by core ID
    cores.sort_by_key(|c| c.core_id);

    Ok((StatusCode::OK, Json(CoretimeOverviewResponse { at, cores })).into_response())
}

// ============================================================================
// Helper Functions - Core Type Determination
// ============================================================================

/// Determines the core type based on reservations and leases.
fn determine_core_type(
    para_id: &str,
    system_paras: &[String],
    reservations: &[ReservationInfo],
    lease_map: &std::collections::HashMap<String, u32>,
) -> CoreType {
    if system_paras.contains(&para_id.to_string()) {
        // It's in reservations
        if para_id == TASK_POOL {
            CoreType {
                condition: CORE_TYPE_ONDEMAND.to_string(),
                details: None,
            }
        } else {
            // Find the mask for this reservation
            let mask = reservations
                .iter()
                .find(|r| r.task == para_id)
                .map(|r| r.mask.clone());
            CoreType {
                condition: CORE_TYPE_RESERVATION.to_string(),
                details: mask.map(|m| CoreTypeDetails {
                    mask: Some(m),
                    until: None,
                }),
            }
        }
    } else if let Some(&until) = lease_map.get(para_id) {
        // It's a lease
        CoreType {
            condition: CORE_TYPE_LEASE.to_string(),
            details: Some(CoreTypeDetails {
                mask: None,
                until: Some(until),
            }),
        }
    } else {
        // Default to bulk
        CoreType {
            condition: CORE_TYPE_BULK.to_string(),
            details: None,
        }
    }
}

// ============================================================================
// Helper Functions - Workload/Workplan Processing
// ============================================================================

/// Extracts workload info from schedule items.
fn extract_workload_info(items: &[ScheduleItem]) -> WorkloadInfo {
    if items.is_empty() {
        return WorkloadInfo {
            is_pool: false,
            is_task: false,
            mask: String::new(),
            task: String::new(),
        };
    }

    let first = &items[0];
    let mask = format!("0x{}", hex::encode(first.mask));
    let (is_pool, is_task, task) = match &first.assignment {
        CoreAssignment::Idle => (false, false, String::new()),
        CoreAssignment::Pool => (true, false, TASK_POOL.to_string()),
        CoreAssignment::Task(id) => (false, true, id.to_string()),
    };

    WorkloadInfo {
        is_pool,
        is_task,
        mask,
        task,
    }
}

/// Converts a WorkplanWithSchedule to a WorkplanEntry for the response.
fn workplan_to_entry(wp: &WorkplanWithSchedule) -> WorkplanEntry {
    WorkplanEntry {
        core: wp.core,
        timeslice: wp.timeslice,
        info: wp
            .items
            .iter()
            .map(schedule_item_to_workload_info)
            .collect(),
    }
}

/// Converts a ScheduleItem to WorkloadInfo.
fn schedule_item_to_workload_info(item: &ScheduleItem) -> WorkloadInfo {
    let mask = format!("0x{}", hex::encode(item.mask));
    let (is_pool, is_task, task) = match &item.assignment {
        CoreAssignment::Idle => (false, false, String::new()),
        CoreAssignment::Pool => (true, false, TASK_POOL.to_string()),
        CoreAssignment::Task(id) => (false, true, id.to_string()),
    };

    WorkloadInfo {
        is_pool,
        is_task,
        mask,
        task,
    }
}

// ============================================================================
// Helper Functions - Data Fetching (specific to overview endpoint)
// ============================================================================

/// Fetches all workload entries from Broker::Workload storage with full schedule data.
///
/// Uses DecodeAsType for efficient typed decoding (no intermediate scale_value::Value).
async fn fetch_workloads_full(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<WorkloadWithSchedule>, CoretimeError> {
    let workload_addr = subxt::dynamic::storage::<(u16,), Vec<ScheduleItem>>("Broker", "Workload");

    let mut workloads = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(workload_addr, ())
        .await
        .map_err(|e| CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Workload",
            details: e.to_string(),
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating workload: {:?}", e);
                continue;
            }
        };

        // Extract core from key using subxt's structured key API
        let core: u32 = match entry
            .key()
            .ok()
            .and_then(|k| k.part(0))
            .and_then(|p| p.decode_as::<u16>().ok().flatten())
        {
            Some(c) => c as u32,
            None => continue,
        };

        // Decode workload value as Vec<ScheduleItem> using DecodeAsType
        let items = match entry.value().decode_as::<Vec<ScheduleItem>>() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to decode workload for core {}: {:?}", core, e);
                Vec::new()
            }
        };

        workloads.push(WorkloadWithSchedule { core, items });
    }

    // Sort by core
    workloads.sort_by_key(|w| w.core);

    Ok(workloads)
}

/// Fetches all workplan entries from Broker::Workplan storage.
///
/// Note: Workplan is a StorageMap with a tuple key (Timeslice, CoreIndex) and OptionQuery value.
/// Pallet definition: StorageMap<_, Twox64Concat, (Timeslice, CoreIndex), Schedule, OptionQuery>
async fn fetch_workplans(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<WorkplanWithSchedule>, CoretimeError> {
    // The key is a tuple (Timeslice, CoreIndex) = (u32, u16)
    // For subxt, we specify it as (u32, u16) and access parts separately
    let workplan_addr =
        subxt::dynamic::storage::<(u32, u16), Vec<ScheduleItem>>("Broker", "Workplan");

    let mut workplans = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(workplan_addr, ())
        .await
        .map_err(|e| CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Workplan",
            details: e.to_string(),
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating workplan: {:?}", e);
                continue;
            }
        };

        // Extract (timeslice, core) from key
        let key = match entry.key() {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("Failed to parse workplan key: {:?}", e);
                continue;
            }
        };

        // Try to decode as tuple first (single key component)
        let (timeslice, core): (u32, u32) = if let Some((t, c)) = key
            .part(0)
            .and_then(|p| p.decode_as::<(u32, u16)>().ok().flatten())
        {
            (t, c as u32)
        } else {
            // Fallback: try as separate key parts (in case subxt treats tuple keys differently)
            let timeslice = match key
                .part(0)
                .and_then(|p| p.decode_as::<u32>().ok().flatten())
            {
                Some(t) => t,
                None => {
                    tracing::warn!("Failed to decode workplan timeslice");
                    continue;
                }
            };
            let core = match key
                .part(1)
                .and_then(|p| p.decode_as::<u16>().ok().flatten())
            {
                Some(c) => c as u32,
                None => {
                    tracing::warn!("Failed to decode workplan core");
                    continue;
                }
            };
            (timeslice, core)
        };

        // Decode workplan value using DecodeAsType
        // Try Vec<ScheduleItem> first, then Option<Vec<ScheduleItem>> as fallback
        let items = match entry.value().decode_as::<Vec<ScheduleItem>>() {
            Ok(v) => v,
            Err(_) => {
                // OptionQuery might wrap the value - try Option<Vec<ScheduleItem>>
                match entry.value().decode_as::<Option<Vec<ScheduleItem>>>() {
                    Ok(Some(v)) => v,
                    Ok(None) => Vec::new(),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to decode workplan for timeslice {}, core {}: {:?}",
                            timeslice,
                            core,
                            e
                        );
                        Vec::new()
                    }
                }
            }
        };

        // Only add non-empty workplans
        if !items.is_empty() {
            workplans.push(WorkplanWithSchedule {
                core,
                timeslice,
                items,
            });
        }
    }

    // Sort by core, then timeslice
    workplans.sort_by(|a, b| a.core.cmp(&b.core).then(a.timeslice.cmp(&b.timeslice)));

    Ok(workplans)
}

// ============================================================================
// Helper Functions - Relay Chain Data Fetching
// ============================================================================

/// Fetches all core descriptors from CoretimeAssignmentProvider::CoreDescriptors storage.
///
/// Note: CoreDescriptors uses Twox256 hasher which is opaque - the key cannot be extracted
/// from the storage key. We query specific core indices directly instead of iterating.
///
/// Queries are sent in parallel batches to minimize round-trip latency. Each batch fires
/// BATCH_SIZE concurrent RPC requests. If an entire batch returns only empty descriptors
/// (after we've already found some), we stop — no more cores exist beyond that point.
async fn fetch_core_descriptors(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<(u32, RelayCoreDescriptorRaw)>, CoretimeError> {
    // Batch size for parallel queries. Sends this many concurrent RPC requests per round.
    // Typical relay chains have ~80-100 active cores, so 2-3 batches suffice.
    const BATCH_SIZE: u32 = 50;
    // Safety ceiling — stop even if batches never come back fully empty.
    const MAX_CORES: u32 = 500;

    let addr = subxt::dynamic::storage::<(u32,), RelayCoreDescriptorRaw>(
        "CoretimeAssignmentProvider",
        "CoreDescriptors",
    );

    let mut descriptors = Vec::new();
    let mut batch_start = 0u32;

    loop {
        let batch_end = (batch_start + BATCH_SIZE).min(MAX_CORES);

        // Fire all queries in this batch concurrently (batch is already
        // bounded by BATCH_SIZE so no extra concurrency limiting needed)
        let futures: Vec<_> = (batch_start..batch_end)
            .map(|core_idx| {
                let addr = addr.clone();
                async move {
                    let result = client_at_block.storage().fetch(addr, (core_idx,)).await;
                    (core_idx, result)
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        let mut batch_found_any = false;
        for (core_idx, result) in results {
            match result {
                Ok(value) => match value.decode_as::<RelayCoreDescriptorRaw>() {
                    Ok(descriptor) => {
                        // CoreDescriptors uses ValueQuery: non-existent cores return
                        // a default descriptor with queue: None, current_work: None.
                        let is_empty =
                            descriptor.current_work.is_none() && descriptor.queue.is_none();
                        if !is_empty {
                            batch_found_any = true;
                            descriptors.push((core_idx, descriptor));
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to decode core descriptor for core {}: {:?}",
                            core_idx,
                            e
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Error fetching core descriptor for core {}: {:?}",
                        core_idx,
                        e
                    );
                }
            }
        }

        batch_start = batch_end;

        // Stop if this entire batch was empty (after we've found at least some cores)
        // or we've reached the safety ceiling.
        if (!batch_found_any && !descriptors.is_empty()) || batch_start >= MAX_CORES {
            break;
        }
    }

    // Sort by core index (parallel results may arrive out of order)
    descriptors.sort_by_key(|(core, _)| *core);

    Ok(descriptors)
}

/// Fetches core schedules from CoretimeAssignmentProvider::CoreSchedules storage.
///
/// Note: CoreSchedules uses Twox256 hasher with key (BlockNumber, CoreIndex).
/// Since we can't iterate opaque hashers and don't know which block numbers have schedules,
/// we return an empty array for now. The main scheduling info is in CoreDescriptors.current_work.
///
/// TODO: Could be enhanced by using queue.first/queue.last from CoreDescriptors to query specific schedules.
async fn fetch_core_schedules(
    _client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<serde_json::Value>, CoretimeError> {
    // CoreSchedules uses Twox256 which is opaque - we can't extract keys from iteration.
    // The schedules would need to be queried by specific (block_number, core_index) pairs,
    // which we could derive from the queue.first/queue.last in each CoreDescriptor.
    // For now, return empty as the main info is in CoreDescriptors.current_work.
    tracing::debug!("CoreSchedules uses opaque hasher - returning empty array");
    Ok(Vec::new())
}

/// Fetches all parachain lifecycles from Paras::ParaLifecycles storage.
async fn fetch_para_lifecycles(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<ParaLifecycle>, CoretimeError> {
    // Check if the Paras pallet exists
    if !has_paras_pallet(client_at_block) {
        return Ok(Vec::new());
    }

    let lifecycles_addr =
        subxt::dynamic::storage::<(u32,), ParaLifecycleType>("Paras", "ParaLifecycles");

    let mut lifecycles = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(lifecycles_addr, ())
        .await
        .map_err(|e| CoretimeError::StorageIterationError {
            pallet: "Paras",
            entry: "ParaLifecycles",
            details: e.to_string(),
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating para lifecycles: {:?}", e);
                continue;
            }
        };

        // Extract para_id from key
        let para_id: u32 = match entry
            .key()
            .ok()
            .and_then(|k| k.part(0))
            .and_then(|p| p.decode_as::<u32>().ok().flatten())
        {
            Some(id) => id,
            None => continue,
        };

        // Decode the lifecycle type using DecodeAsType
        let lifecycle_type = entry
            .value()
            .decode_as::<ParaLifecycleType>()
            .ok()
            .map(|lt| lt.as_str().to_string());

        lifecycles.push(ParaLifecycle {
            para_id,
            lifecycle_type,
        });
    }

    Ok(lifecycles)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::coretime::common::CORE_MASK_SIZE;

    // ------------------------------------------------------------------------
    // WorkloadInfo tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_workload_info_serialization() {
        let info = WorkloadInfo {
            is_pool: false,
            is_task: true,
            mask: "0xffffffffffffffffffff".to_string(),
            task: "2000".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"isPool\":false"));
        assert!(json.contains("\"isTask\":true"));
        assert!(json.contains("\"mask\":\"0xffffffffffffffffffff\""));
        assert!(json.contains("\"task\":\"2000\""));
    }

    #[test]
    fn test_workload_info_pool() {
        let info = WorkloadInfo {
            is_pool: true,
            is_task: false,
            mask: "0xffffffffffffffffffff".to_string(),
            task: TASK_POOL.to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"isPool\":true"));
        assert!(json.contains(&format!("\"task\":\"{}\"", TASK_POOL)));
    }

    // ------------------------------------------------------------------------
    // CoreType tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_core_type_lease_serialization() {
        let ct = CoreType {
            condition: CORE_TYPE_LEASE.to_string(),
            details: Some(CoreTypeDetails {
                mask: None,
                until: Some(12345),
            }),
        };

        let json = serde_json::to_string(&ct).unwrap();
        assert!(json.contains(&format!("\"condition\":\"{}\"", CORE_TYPE_LEASE)));
        assert!(json.contains("\"until\":12345"));
        assert!(!json.contains("\"mask\""));
    }

    #[test]
    fn test_core_type_reservation_serialization() {
        let ct = CoreType {
            condition: CORE_TYPE_RESERVATION.to_string(),
            details: Some(CoreTypeDetails {
                mask: Some("0xffffffffffffffffffff".to_string()),
                until: None,
            }),
        };

        let json = serde_json::to_string(&ct).unwrap();
        assert!(json.contains(&format!("\"condition\":\"{}\"", CORE_TYPE_RESERVATION)));
        assert!(json.contains("\"mask\":\"0xffffffffffffffffffff\""));
        assert!(!json.contains("\"until\""));
    }

    #[test]
    fn test_core_type_bulk_serialization() {
        let ct = CoreType {
            condition: CORE_TYPE_BULK.to_string(),
            details: None,
        };

        let json = serde_json::to_string(&ct).unwrap();
        assert!(json.contains(&format!("\"condition\":\"{}\"", CORE_TYPE_BULK)));
        assert!(!json.contains("\"details\""));
    }

    #[test]
    fn test_core_type_ondemand_serialization() {
        let ct = CoreType {
            condition: CORE_TYPE_ONDEMAND.to_string(),
            details: None,
        };

        let json = serde_json::to_string(&ct).unwrap();
        assert!(json.contains(&format!("\"condition\":\"{}\"", CORE_TYPE_ONDEMAND)));
    }

    // ------------------------------------------------------------------------
    // determine_core_type tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_determine_core_type_ondemand() {
        let system_paras = vec![TASK_POOL.to_string()];
        let reservations = vec![ReservationInfo {
            task: TASK_POOL.to_string(),
            mask: "0xff".to_string(),
        }];
        let lease_map = std::collections::HashMap::new();

        let result = determine_core_type(TASK_POOL, &system_paras, &reservations, &lease_map);
        assert_eq!(result.condition, CORE_TYPE_ONDEMAND);
        assert!(result.details.is_none());
    }

    #[test]
    fn test_determine_core_type_reservation() {
        let system_paras = vec!["1000".to_string()];
        let reservations = vec![ReservationInfo {
            task: "1000".to_string(),
            mask: "0xffffffffffffffffffff".to_string(),
        }];
        let lease_map = std::collections::HashMap::new();

        let result = determine_core_type("1000", &system_paras, &reservations, &lease_map);
        assert_eq!(result.condition, CORE_TYPE_RESERVATION);
        assert!(result.details.is_some());
        assert_eq!(
            result.details.unwrap().mask,
            Some("0xffffffffffffffffffff".to_string())
        );
    }

    #[test]
    fn test_determine_core_type_lease() {
        let system_paras = vec![];
        let reservations = vec![];
        let mut lease_map = std::collections::HashMap::new();
        lease_map.insert("2000".to_string(), 12345u32);

        let result = determine_core_type("2000", &system_paras, &reservations, &lease_map);
        assert_eq!(result.condition, CORE_TYPE_LEASE);
        assert!(result.details.is_some());
        assert_eq!(result.details.unwrap().until, Some(12345));
    }

    #[test]
    fn test_determine_core_type_bulk() {
        let system_paras = vec![];
        let reservations = vec![];
        let lease_map = std::collections::HashMap::new();

        let result = determine_core_type("3000", &system_paras, &reservations, &lease_map);
        assert_eq!(result.condition, CORE_TYPE_BULK);
        assert!(result.details.is_none());
    }

    // ------------------------------------------------------------------------
    // extract_workload_info tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_extract_workload_info_empty() {
        let items: Vec<ScheduleItem> = vec![];
        let result = extract_workload_info(&items);
        assert!(!result.is_pool);
        assert!(!result.is_task);
        assert!(result.mask.is_empty());
        assert!(result.task.is_empty());
    }

    #[test]
    fn test_extract_workload_info_task() {
        let items = vec![ScheduleItem {
            mask: [0xFF; CORE_MASK_SIZE],
            assignment: CoreAssignment::Task(2000),
        }];
        let result = extract_workload_info(&items);
        assert!(!result.is_pool);
        assert!(result.is_task);
        assert_eq!(result.mask, "0xffffffffffffffffffff");
        assert_eq!(result.task, "2000");
    }

    #[test]
    fn test_extract_workload_info_pool() {
        let items = vec![ScheduleItem {
            mask: [0xAA; CORE_MASK_SIZE],
            assignment: CoreAssignment::Pool,
        }];
        let result = extract_workload_info(&items);
        assert!(result.is_pool);
        assert!(!result.is_task);
        assert_eq!(result.task, TASK_POOL);
    }

    #[test]
    fn test_extract_workload_info_idle() {
        let items = vec![ScheduleItem {
            mask: [0xFF; CORE_MASK_SIZE],
            assignment: CoreAssignment::Idle,
        }];
        let result = extract_workload_info(&items);
        assert!(!result.is_pool);
        assert!(!result.is_task);
        assert!(result.task.is_empty());
    }

    // ------------------------------------------------------------------------
    // CoreInfo serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_core_info_serialization() {
        let info = CoreInfo {
            core_id: 5,
            para_id: "2000".to_string(),
            workload: WorkloadInfo {
                is_pool: false,
                is_task: true,
                mask: "0xffffffffffffffffffff".to_string(),
                task: "2000".to_string(),
            },
            workplan: vec![],
            core_type: CoreType {
                condition: CORE_TYPE_LEASE.to_string(),
                details: Some(CoreTypeDetails {
                    mask: None,
                    until: Some(12345),
                }),
            },
            regions: vec![],
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"coreId\":5"));
        assert!(json.contains("\"paraId\":\"2000\""));
        assert!(json.contains("\"workload\""));
        assert!(json.contains("\"type\""));
        assert!(json.contains(&format!("\"condition\":\"{}\"", CORE_TYPE_LEASE)));
    }

    // ------------------------------------------------------------------------
    // Response serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_overview_response_serialization() {
        let response = CoretimeOverviewResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            cores: vec![CoreInfo {
                core_id: 0,
                para_id: "1000".to_string(),
                workload: WorkloadInfo {
                    is_pool: false,
                    is_task: true,
                    mask: "0xffffffffffffffffffff".to_string(),
                    task: "1000".to_string(),
                },
                workplan: vec![],
                core_type: CoreType {
                    condition: CORE_TYPE_RESERVATION.to_string(),
                    details: Some(CoreTypeDetails {
                        mask: Some("0xffffffffffffffffffff".to_string()),
                        until: None,
                    }),
                },
                regions: vec![],
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"cores\""));
        assert!(json.contains("\"coreId\":0"));
    }

    // ------------------------------------------------------------------------
    // Relay chain core filtering tests
    // ------------------------------------------------------------------------

    /// Helper to build a RelayCoreDescriptor for testing the filter logic.
    fn make_relay_core(
        core: u32,
        para_id: Option<u32>,
        is_task: bool,
        is_pool: bool,
    ) -> RelayCoreDescriptor {
        let task = match (is_task, is_pool) {
            (true, _) => para_id.map(|id| id.to_string()).unwrap_or_default(),
            (_, true) => TASK_POOL.to_string(),
            _ => String::new(),
        };
        RelayCoreDescriptor {
            core,
            para_id,
            lifecycle: para_id.map(|_| "Parachain".to_string()),
            info: RelayCoreDescriptorInfo {
                current_work: RelayCurrentWork {
                    assignments: vec![RelayAssignmentInfo {
                        task,
                        ratio: 57600,
                        remaining: 57600,
                        is_task,
                        is_pool,
                    }],
                    end_hint: None,
                    pos: 0,
                    step: 57600,
                },
                queue: RelayQueueDescriptor {
                    first: "0".to_string(),
                    last: "0".to_string(),
                },
            },
        }
    }

    fn make_empty_relay_core(core: u32) -> RelayCoreDescriptor {
        RelayCoreDescriptor {
            core,
            para_id: None,
            lifecycle: None,
            info: RelayCoreDescriptorInfo {
                current_work: RelayCurrentWork {
                    assignments: vec![],
                    end_hint: None,
                    pos: 0,
                    step: 0,
                },
                queue: RelayQueueDescriptor {
                    first: "0".to_string(),
                    last: "0".to_string(),
                },
            },
        }
    }

    /// Applies the same filter+dedup logic used in handle_relay_chain_overview.
    /// Keeps cores with paraId + lifecycle, deduplicated by paraId.
    fn filter_relay_cores(cores: Vec<RelayCoreDescriptor>) -> Vec<RelayCoreDescriptor> {
        let mut seen = std::collections::HashSet::new();
        cores
            .into_iter()
            .filter(|core| match core.para_id {
                Some(pid) if core.lifecycle.is_some() => seen.insert(pid),
                _ => false,
            })
            .collect()
    }

    #[test]
    fn test_relay_filter_keeps_task_cores_with_lifecycle() {
        let core = make_relay_core(0, Some(1000), true, false);
        let filtered = filter_relay_cores(vec![core]);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_relay_filter_excludes_pool_cores() {
        // Pool cores have no paraId, so they don't match any paraLifecycle.
        // Sidecar also excludes them (iterates paraLifecycles, not core types).
        let core = make_relay_core(1, None, false, true);
        let filtered = filter_relay_cores(vec![core]);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_relay_filter_excludes_idle_cores() {
        let core = make_relay_core(2, None, false, false);
        let filtered = filter_relay_cores(vec![core]);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_relay_filter_excludes_empty_cores() {
        let core = make_empty_relay_core(3);
        let filtered = filter_relay_cores(vec![core]);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_relay_filter_excludes_task_without_lifecycle() {
        let mut core = make_relay_core(5, Some(9999), true, false);
        core.lifecycle = None;
        let filtered = filter_relay_cores(vec![core]);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_relay_filter_deduplicates_by_para_id() {
        // Multiple cores assigned to the same paraId: only the first is kept.
        // This matches sidecar's .find() behavior.
        let cores = vec![
            make_relay_core(0, Some(1000), true, false),
            make_relay_core(1, Some(1000), true, false), // Duplicate paraId
            make_relay_core(2, Some(1000), true, false), // Duplicate paraId
        ];
        let filtered = filter_relay_cores(cores);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].core, 0); // First core kept
    }

    #[test]
    fn test_relay_filter_mixed_cores() {
        let mut task_no_lifecycle = make_relay_core(6, Some(9999), true, false);
        task_no_lifecycle.lifecycle = None;

        let cores = vec![
            make_relay_core(0, Some(1000), true, false), // Task + lifecycle - keep
            make_relay_core(1, None, false, false),      // Idle - filter
            make_relay_core(2, None, false, true),       // Pool - filter (no paraId)
            make_empty_relay_core(3),                    // Empty - filter
            make_relay_core(4, Some(2000), true, false), // Task + lifecycle - keep
            make_relay_core(5, Some(1000), true, false), // Duplicate paraId 1000 - filter
            task_no_lifecycle,                           // Task no lifecycle - filter
        ];

        let filtered = filter_relay_cores(cores);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].core, 0);
        assert_eq!(filtered[0].para_id, Some(1000));
        assert_eq!(filtered[1].core, 4);
        assert_eq!(filtered[1].para_id, Some(2000));
    }

    #[test]
    fn test_relay_filter_preserves_core_index_ordering() {
        let cores = vec![
            make_relay_core(5, Some(3000), true, false),
            make_relay_core(0, Some(1000), true, false),
            make_relay_core(10, Some(2000), true, false),
        ];

        let filtered = filter_relay_cores(cores);

        // Order preserved as-is (caller sorts by core index before filtering)
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].core, 5);
        assert_eq!(filtered[1].core, 0);
        assert_eq!(filtered[2].core, 10);
    }
}
