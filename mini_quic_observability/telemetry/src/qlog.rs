// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Observability: QLOG Event Serializer
// RFC draft-ietf-quic-qlog JSON-seq format for Wireshark/qvis compatibility

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// QLOG version (RFC draft-ietf-quic-qlog)
const QLOG_VERSION: &str = "0.3";

/// QLOG trace specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QlogTrace {
    pub vantage_point: VantagePoint,
    pub title: String,
    pub description: Option<String>,
    pub configuration: TraceConfiguration,
    pub events: Vec<QlogEvent>,
}

/// Vantage point perspective
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VantagePoint {
    pub name: String,
    pub role: String, // "client" or "server"
    pub type_: String, // "local", "remote", or "unknown"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_type: Option<String>,
}

/// Trace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfiguration {
    pub time_offset: u64, // Offset in microseconds from epoch
    pub time_units: String, // "ms", "us", etc.
    pub original_uris: Option<Vec<String>>,
}

/// Individual QLOG event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QlogEvent {
    pub relative_time: u64, // Microseconds from trace start
    pub category: String,
    pub event: String,
    pub trigger: Option<String>,
    pub data: serde_json::Value,
}

impl QlogEvent {
    /// Create packet sent event
    pub fn packet_sent(
        relative_time: u64,
        packet_number: u64,
        packet_length: u64,
        encryption_level: &str,
    ) -> Self {
        QlogEvent {
            relative_time,
            category: "transport".to_string(),
            event: "packet_sent".to_string(),
            trigger: None,
            data: serde_json::json!({
                "packet_number": packet_number,
                "length": packet_length,
                "encryption_level": encryption_level,
                "packet_type": "short_header"
            }),
        }
    }

    /// Create packet received event
    pub fn packet_received(
        relative_time: u64,
        packet_number: u64,
        packet_length: u64,
        encryption_level: &str,
    ) -> Self {
        QlogEvent {
            relative_time,
            category: "transport".to_string(),
            event: "packet_received".to_string(),
            trigger: None,
            data: serde_json::json!({
                "packet_number": packet_number,
                "length": packet_length,
                "encryption_level": encryption_level,
            }),
        }
    }

    /// Create loss detection event
    pub fn loss_detected(
        relative_time: u64,
        lost_packets: Vec<u64>,
        detection_method: &str,
    ) -> Self {
        QlogEvent {
            relative_time,
            category: "recovery".to_string(),
            event: "packets_lost".to_string(),
            trigger: Some(detection_method.to_string()),
            data: serde_json::json!({
                "packet_numbers": lost_packets,
                "detection_method": detection_method,
            }),
        }
    }

    /// Create congestion control event
    pub fn congestion_control_update(
        relative_time: u64,
        state: &str,
        cwnd_bytes: u64,
        ssthresh_bytes: Option<u64>,
        trigger: &str,
    ) -> Self {
        QlogEvent {
            relative_time,
            category: "recovery".to_string(),
            event: "congestion_state_updated".to_string(),
            trigger: Some(trigger.to_string()),
            data: serde_json::json!({
                "state": state,
                "congestion_window": cwnd_bytes,
                "ssthresh": ssthresh_bytes,
            }),
        }
    }

    /// Create RTT update event
    pub fn rtt_updated(
        relative_time: u64,
        latest_rtt_us: u64,
        smoothed_rtt_us: u64,
        min_rtt_us: Option<u64>,
    ) -> Self {
        QlogEvent {
            relative_time,
            category: "recovery".to_string(),
            event: "rtt_updated".to_string(),
            trigger: None,
            data: serde_json::json!({
                "latest_rtt": latest_rtt_us,
                "smoothed_rtt": smoothed_rtt_us,
                "min_rtt": min_rtt_us,
            }),
        }
    }

    /// Create stream created event
    pub fn stream_created(relative_time: u64, stream_id: u64, direction: &str) -> Self {
        QlogEvent {
            relative_time,
            category: "http".to_string(),
            event: "stream_created".to_string(),
            trigger: None,
            data: serde_json::json!({
                "stream_id": stream_id,
                "direction": direction,
            }),
        }
    }

    /// Create flow control update
    pub fn flow_control_updated(
        relative_time: u64,
        stream_id: Option<u64>,
        max_data: u64,
        data_received: u64,
    ) -> Self {
        QlogEvent {
            relative_time,
            category: "transport".to_string(),
            event: "max_data_updated".to_string(),
            trigger: None,
            data: serde_json::json!({
                "stream_id": stream_id,
                "max_data": max_data,
                "data_received": data_received,
            }),
        }
    }
}

/// QLOG exporter: Writes JSON-seq formatted events
pub struct QlogExporter {
    trace_start_time: SystemTime,
    events: Vec<QlogEvent>,
    vantage_point: VantagePoint,
}

impl QlogExporter {
    pub fn new(role: &str) -> Self {
        QlogExporter {
            trace_start_time: SystemTime::now(),
            events: Vec::new(),
            vantage_point: VantagePoint {
                name: format!("mini_quic_{}", role),
                role: role.to_string(),
                type_: "local".to_string(),
                flow_type: None,
            },
        }
    }

    /// Add event to trace
    pub fn add_event(&mut self, event: QlogEvent) {
        self.events.push(event);
    }

    /// Compute relative time from trace start
    fn relative_time_us(&self) -> u64 {
        self.trace_start_time
            .elapsed()
            .unwrap_or_default()
            .as_micros() as u64
    }

    /// Export as JSON-seq (NDJSON format)
    pub fn export_jsonseq(&self) -> String {
        let mut output = String::new();

        // Write each event as JSON followed by newline (JSON-seq)
        for event in &self.events {
            if let Ok(json) = serde_json::to_string(event) {
                output.push_str(&json);
                output.push('\n');
            }
        }

        output
    }

    /// Export full trace
    pub fn export_full_trace(&self, title: &str) -> String {
        let trace_time_offset = self
            .trace_start_time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let trace = QlogTrace {
            vantage_point: self.vantage_point.clone(),
            title: title.to_string(),
            description: Some("Mini-QUIC observability trace".to_string()),
            configuration: TraceConfiguration {
                time_offset: trace_time_offset,
                time_units: "us".to_string(),
                original_uris: None,
            },
            events: self.events.clone(),
        };

        serde_json::to_string_pretty(&trace).unwrap_or_default()
    }

    /// Export for qvis consumption (includes metadata)
    pub fn export_for_qvis(&self) -> serde_json::Value {
        serde_json::json!({
            "version": QLOG_VERSION,
            "traces": [{
                "vantage_point": self.vantage_point,
                "events": self.events,
                "configuration": {
                    "time_units": "us"
                }
            }]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qlog_event_creation() {
        let event = QlogEvent::packet_sent(1000, 0, 1200, "Initial");
        assert_eq!(event.category, "transport");
        assert_eq!(event.event, "packet_sent");
        assert_eq!(event.relative_time, 1000);
    }

    #[test]
    fn test_qlog_exporter() {
        let mut exporter = QlogExporter::new("client");
        exporter.add_event(QlogEvent::packet_sent(0, 0, 1200, "Initial"));
        exporter.add_event(QlogEvent::rtt_updated(100, 50000, 50000, None));

        let jsonseq = exporter.export_jsonseq();
        assert!(jsonseq.contains("packet_sent"));
        assert!(jsonseq.contains("rtt_updated"));
    }

    #[test]
    fn test_qlog_loss_detection() {
        let event =
            QlogEvent::loss_detected(500, vec![0, 1, 2], "packet_threshold");
        assert_eq!(event.event, "packets_lost");
    }
}
