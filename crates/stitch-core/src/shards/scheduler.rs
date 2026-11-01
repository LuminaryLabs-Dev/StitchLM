use super::definition::*;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub operation: Operation,
    pub kernel: String,
    pub units: u64,
    pub execution_ms: f64,
    pub loading_ms: f64,
    pub conversion_ms: f64,
    pub result_return_ms: f64,
    pub measured_at_ms: u64,
    pub samples: u32,
    pub uncertainty_ms: f64,
    pub end_to_end_ms: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceCapabilities {
    pub id: String,
    pub version: u64,
    pub backend: String,
    pub capabilities: Vec<String>,
    pub kernels: Vec<String>,
    pub available_memory: u64,
    pub runtime_overhead: u64,
    pub current_workload: u32,
    pub maximum_in_flight: u32,
    pub network_latency_ms: Option<f64>,
    pub bandwidth_bytes_per_ms: Option<f64>,
    pub queue_delay_ms: Option<f64>,
    pub measured_at_ms: u64,
    pub measurements: Vec<Measurement>,
    pub thermal_state: Option<String>,
    pub battery_percent: Option<f64>,
    pub background_execution: Option<bool>,
    pub online: bool,
    pub heartbeat_ms: u64,
    pub allowed_sensitivities: Vec<Sensitivity>,
}
impl DeviceCapabilities {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            identity(&self.id)
                && self.version > 0
                && ["cpu", "wasm", "gpu"].contains(&self.backend.as_str())
                && self.maximum_in_flight > 0
                && self.maximum_in_flight <= 64,
            "invalid device identity/capacity"
        );
        ensure!(
            [
                self.network_latency_ms,
                self.bandwidth_bytes_per_ms,
                self.queue_delay_ms
            ]
            .iter()
            .all(|x| x.is_none_or(|v| v.is_finite() && v >= 0.))
                && self.bandwidth_bytes_per_ms.is_none_or(|v| v > 0.),
            "invalid network measurements"
        );
        ensure!(
            self.battery_percent
                .is_none_or(|x| x.is_finite() && (0.0..=100.0).contains(&x)),
            "invalid battery observation"
        );
        for m in &self.measurements {
            ensure!(
                m.end_to_end_ms.is_none_or(|v| v.is_finite() && v >= 0.),
                "invalid end-to-end timing"
            );
            ensure!(
                m.units > 0
                    && m.samples > 0
                    && [
                        m.execution_ms,
                        m.loading_ms,
                        m.conversion_ms,
                        m.result_return_ms,
                        m.uncertainty_ms
                    ]
                    .iter()
                    .all(|x| x.is_finite() && *x >= 0.),
                "invalid operation measurement"
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    Latency,
    Capacity,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement {
    pub device_id: String,
    pub predicted_ms: Option<f64>,
    pub expected_saved_ms: Option<f64>,
    pub measured: bool,
    pub reason: String,
}
pub fn choose_device(
    definition: &ShardDefinition,
    devices: &[DeviceCapabilities],
    units: u64,
    host_ms: Option<f64>,
    now: u64,
    maximum_age_ms: u64,
    objective: Objective,
) -> Result<Option<Placement>> {
    definition.validate()?;
    ensure!(
        units > 0 && maximum_age_ms > 0 && host_ms.is_none_or(|x| x.is_finite() && x >= 0.),
        "invalid scheduling input"
    );
    let mut candidates = Vec::new();
    for device in devices {
        device.validate()?;
        let cost = definition
            .budget
            .estimated_peak
            .checked_add(device.runtime_overhead)
            .ok_or_else(|| anyhow::anyhow!("device budget overflow"))?;
        if !device.online
            || now.saturating_sub(device.heartbeat_ms) > maximum_age_ms
            || device.current_workload >= device.maximum_in_flight
            || device.available_memory < cost
            || cost > definition.budget.memory_limit
            || !definition.ownership.allowed_devices.contains(&device.id)
            || !definition
                .storage
                .approved_destinations
                .contains(&device.id)
            || !device
                .allowed_sensitivities
                .contains(&definition.ownership.sensitivity)
            || !definition
                .computation
                .capabilities
                .iter()
                .all(|c| device.capabilities.contains(c))
            || !definition
                .computation
                .kernels
                .iter()
                .any(|k| device.kernels.contains(k))
        {
            continue;
        }
        if device.background_execution == Some(false)
            && definition
                .computation
                .capabilities
                .iter()
                .any(|x| x == "background")
        {
            continue;
        }
        let measurement = device
            .measurements
            .iter()
            .filter(|m| {
                m.operation == definition.computation.operation
                    && definition.computation.kernels.contains(&m.kernel)
                    && now.saturating_sub(m.measured_at_ms) <= maximum_age_ms
            })
            .max_by_key(|m| m.measured_at_ms);
        let predicted = measurement.and_then(|m| {
            if let Some(total) = m.end_to_end_ms {
                Some(total * units as f64 / m.units as f64 + m.uncertainty_ms)
            } else {
                Some(
                    m.execution_ms * units as f64 / m.units as f64
                        + m.loading_ms
                        + m.conversion_ms
                        + m.result_return_ms
                        + m.uncertainty_ms
                        + device.network_latency_ms?
                        + device.queue_delay_ms?
                        + definition.storage.transfer_bytes as f64
                            / device.bandwidth_bytes_per_ms?,
                )
            }
        });
        let saved = predicted.zip(host_ms).map(|(p, h)| h - p);
        if objective == Objective::Latency && !saved.is_some_and(|s| s > 0.) {
            continue;
        }
        candidates.push(Placement {
            device_id: device.id.clone(),
            predicted_ms: predicted,
            expected_saved_ms: saved,
            measured: measurement.is_some(),
            reason: if measurement.is_some() {
                "compatible, within budget, measured total cost"
            } else {
                "capacity placement; speed benefit is unmeasured"
            }
            .into(),
        });
    }
    candidates.sort_by(|a, b| {
        a.predicted_ms
            .unwrap_or(f64::INFINITY)
            .total_cmp(&b.predicted_ms.unwrap_or(f64::INFINITY))
            .then(a.device_id.cmp(&b.device_id))
    });
    Ok(candidates.into_iter().next())
}
