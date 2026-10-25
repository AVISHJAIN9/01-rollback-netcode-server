// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Stream & Connection Flow Control (RFC 9000 §4)
// Window credit managers with auto-tuning and backpressure

use std::collections::HashMap;

/// Stream state lifecycle (RFC 9000 §3.4).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StreamState {
    /// Stream open for sending and receiving.
    Open,

    /// Send side finalized; receive side open.
    LocalFinished,

    /// Receive side finalized; send side open.
    RemoteFinished,

    /// Both sides finalized.
    Closed,

    /// Stream reset locally or remotely.
    ResetSent,

    /// Stream reset by peer.
    ResetReceived,
}

/// Connection-level flow control window manager (RFC 9000 §4.1).
#[derive(Clone, Debug)]
pub struct ConnectionFlowControl {
    /// Maximum data the peer can send (advertised limit).
    max_data: u64,

    /// Data received so far from peer.
    data_received: u64,

    /// Flow control window size (initial max_data).
    flow_control_window: u64,

    /// When to send MAX_DATA (auto-tuning threshold).
    auto_tune_threshold: f64, // Advertise new limit when consumed > 50% of window

    /// Last sent MAX_DATA value.
    last_advertised_limit: u64,
}

impl ConnectionFlowControl {
    pub fn new(initial_window: u64) -> Self {
        ConnectionFlowControl {
            max_data: initial_window,
            data_received: 0,
            flow_control_window: initial_window,
            auto_tune_threshold: 0.5,
            last_advertised_limit: initial_window,
        }
    }

    /// Record bytes received from peer.
    ///
    /// Returns `Err` if bytes exceed max_data (flow control violation).
    pub fn on_data_received(&mut self, bytes: u64) -> Result<(), FlowControlError> {
        if self.data_received + bytes > self.max_data {
            return Err(FlowControlError::FlowControlViolation);
        }

        self.data_received += bytes;
        Ok(())
    }

    /// Check if we should advertise a new MAX_DATA frame.
    ///
    /// Returns the new limit if auto-tuning decides to send MAX_DATA.
    pub fn should_advertise_limit(&self) -> Option<u64> {
        let consumed = self.data_received;
        let total_window = self.flow_control_window;

        let consumption_ratio = if total_window > 0 {
            consumed as f64 / total_window as f64
        } else {
            0.0
        };

        if consumption_ratio > self.auto_tune_threshold {
            // Double the window
            let new_limit = (consumed + total_window).saturating_mul(2);

            if new_limit != self.last_advertised_limit {
                Some(new_limit)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Update the advertised limit after sending MAX_DATA.
    pub fn advertise_limit(&mut self, new_limit: u64) {
        self.flow_control_window = new_limit;
        self.last_advertised_limit = new_limit;
    }

    /// Get available send window (how much peer can send).
    pub fn available_window(&self) -> u64 {
        self.max_data.saturating_sub(self.data_received)
    }

    /// Get data received so far.
    pub fn data_received(&self) -> u64 {
        self.data_received
    }

    /// Get max data (peer's limit).
    pub fn max_data(&self) -> u64 {
        self.max_data
    }
}

/// Per-stream flow control window manager.
#[derive(Clone, Debug)]
pub struct StreamFlowControl {
    /// Stream ID for identification.
    stream_id: u64,

    /// Maximum stream data (send limit from peer).
    max_stream_data: u64,

    /// Stream data sent so far.
    stream_data_sent: u64,

    /// Maximum stream data received (our advertised limit).
    max_stream_data_recv: u64,

    /// Stream data received so far.
    stream_data_received: u64,

    /// When to send MAX_STREAM_DATA.
    auto_tune_threshold: f64,

    /// Last advertised limit.
    last_advertised_limit: u64,
}

impl StreamFlowControl {
    pub fn new(stream_id: u64, initial_window: u64) -> Self {
        StreamFlowControl {
            stream_id,
            max_stream_data: initial_window,
            stream_data_sent: 0,
            max_stream_data_recv: initial_window,
            stream_data_received: 0,
            auto_tune_threshold: 0.5,
            last_advertised_limit: initial_window,
        }
    }

    /// Try to send data on stream (check against peer's max_stream_data).
    pub fn can_send(&self, bytes: u64) -> bool {
        self.stream_data_sent + bytes <= self.max_stream_data
    }

    /// Record bytes sent on stream.
    pub fn on_data_sent(&mut self, bytes: u64) -> Result<(), FlowControlError> {
        if !self.can_send(bytes) {
            return Err(FlowControlError::StreamFlowControlViolation);
        }

        self.stream_data_sent += bytes;
        Ok(())
    }

    /// Record bytes received on stream.
    pub fn on_data_received(&mut self, bytes: u64) -> Result<(), FlowControlError> {
        if self.stream_data_received + bytes > self.max_stream_data_recv {
            return Err(FlowControlError::StreamFlowControlViolation);
        }

        self.stream_data_received += bytes;
        Ok(())
    }

    /// Check if we should send MAX_STREAM_DATA.
    pub fn should_advertise_limit(&self) -> Option<u64> {
        let consumed = self.stream_data_received;
        let total_window = self.max_stream_data_recv;

        let consumption_ratio = if total_window > 0 {
            consumed as f64 / total_window as f64
        } else {
            0.0
        };

        if consumption_ratio > self.auto_tune_threshold {
            let new_limit = (consumed + total_window).saturating_mul(2);

            if new_limit != self.last_advertised_limit {
                Some(new_limit)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Update advertised limit.
    pub fn advertise_limit(&mut self, new_limit: u64) {
        self.max_stream_data_recv = new_limit;
        self.last_advertised_limit = new_limit;
    }

    /// Process MAX_STREAM_DATA from peer (update send limit).
    pub fn on_max_stream_data(&mut self, new_limit: u64) {
        self.max_stream_data = self.max_stream_data.max(new_limit);
    }

    /// Get available send window.
    pub fn available_window(&self) -> u64 {
        self.max_stream_data.saturating_sub(self.stream_data_sent)
    }

    /// Get stream data sent.
    pub fn data_sent(&self) -> u64 {
        self.stream_data_sent
    }

    /// Get stream data received.
    pub fn data_received(&self) -> u64 {
        self.stream_data_received
    }

    /// Get stream ID.
    pub fn stream_id(&self) -> u64 {
        self.stream_id
    }
}

/// Flow controller for multiple streams.
#[derive(Clone, Debug)]
pub struct StreamFlowControlManager {
    /// Map of stream ID -> StreamFlowControl.
    streams: HashMap<u64, StreamFlowControl>,

    /// Connection-level flow control.
    conn_flow_control: ConnectionFlowControl,

    /// Initial stream window size.
    initial_stream_window: u64,
}

impl StreamFlowControlManager {
    pub fn new(conn_window: u64, stream_window: u64) -> Self {
        StreamFlowControlManager {
            streams: HashMap::new(),
            conn_flow_control: ConnectionFlowControl::new(conn_window),
            initial_stream_window: stream_window,
        }
    }

    /// Get or create stream flow control.
    pub fn get_or_create(&mut self, stream_id: u64) -> &mut StreamFlowControl {
        self.streams
            .entry(stream_id)
            .or_insert_with(|| StreamFlowControl::new(stream_id, self.initial_stream_window))
    }

    /// Get stream flow control.
    pub fn get(&self, stream_id: u64) -> Option<&StreamFlowControl> {
        self.streams.get(&stream_id)
    }

    /// Get mutable stream flow control.
    pub fn get_mut(&mut self, stream_id: u64) -> Option<&mut StreamFlowControl> {
        self.streams.get_mut(&stream_id)
    }

    /// Remove stream flow control (when stream closes).
    pub fn remove_stream(&mut self, stream_id: u64) {
        self.streams.remove(&stream_id);
    }

    /// Get connection flow control.
    pub fn connection(&self) -> &ConnectionFlowControl {
        &self.conn_flow_control
    }

    /// Get mutable connection flow control.
    pub fn connection_mut(&mut self) -> &mut ConnectionFlowControl {
        &mut self.conn_flow_control
    }

    /// Check if stream can send bytes.
    pub fn can_send_stream(&self, stream_id: u64, bytes: u64) -> bool {
        if self.conn_flow_control.available_window() < bytes {
            return false;
        }

        if let Some(stream_fc) = self.streams.get(&stream_id) {
            stream_fc.can_send(bytes)
        } else {
            false
        }
    }

    /// Record bytes sent on stream and connection.
    pub fn on_stream_data_sent(&mut self, stream_id: u64, bytes: u64) -> Result<(), FlowControlError> {
        self.conn_flow_control.on_data_received(bytes)?;

        if let Some(stream_fc) = self.streams.get_mut(&stream_id) {
            stream_fc.on_data_sent(bytes)?;
        }

        Ok(())
    }

    /// Record bytes received on stream and connection.
    pub fn on_stream_data_received(&mut self, stream_id: u64, bytes: u64) -> Result<(), FlowControlError> {
        self.conn_flow_control.on_data_received(bytes)?;

        let stream_fc = self.get_or_create(stream_id);
        stream_fc.on_data_received(bytes)?;

        Ok(())
    }

    /// Get streams needing MAX_STREAM_DATA advertisement.
    pub fn streams_needing_advertisement(&self) -> Vec<(u64, u64)> {
        self.streams
            .iter()
            .filter_map(|(stream_id, fc)| {
                fc.should_advertise_limit().map(|limit| (*stream_id, limit))
            })
            .collect()
    }

    /// Process peer's MAX_STREAM_DATA.
    pub fn on_max_stream_data(&mut self, stream_id: u64, new_limit: u64) {
        let stream_fc = self.get_or_create(stream_id);
        stream_fc.on_max_stream_data(new_limit);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlowControlError {
    FlowControlViolation,
    StreamFlowControlViolation,
}

impl std::fmt::Display for FlowControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FlowControlViolation => write!(f, "connection flow control violation"),
            Self::StreamFlowControlViolation => write!(f, "stream flow control violation"),
        }
    }
}

impl std::error::Error for FlowControlError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_flow_control_init() {
        let fc = ConnectionFlowControl::new(100000);
        assert_eq!(fc.max_data(), 100000);
        assert_eq!(fc.available_window(), 100000);
    }

    #[test]
    fn test_connection_flow_control_data_received() {
        let mut fc = ConnectionFlowControl::new(100000);

        fc.on_data_received(50000).unwrap();
        assert_eq!(fc.data_received(), 50000);
        assert_eq!(fc.available_window(), 50000);
    }

    #[test]
    fn test_connection_flow_control_violation() {
        let mut fc = ConnectionFlowControl::new(100000);

        assert!(fc.on_data_received(100001).is_err());
    }

    #[test]
    fn test_connection_auto_tune() {
        let mut fc = ConnectionFlowControl::new(100000);

        // Consume 60% of window
        fc.on_data_received(60000).unwrap();

        let new_limit = fc.should_advertise_limit();
        assert!(new_limit.is_some());
        assert!(new_limit.unwrap() > 100000);
    }

    #[test]
    fn test_stream_flow_control_init() {
        let fc = StreamFlowControl::new(0, 50000);
        assert_eq!(fc.stream_id(), 0);
        assert_eq!(fc.available_window(), 50000);
    }

    #[test]
    fn test_stream_flow_control_can_send() {
        let fc = StreamFlowControl::new(0, 50000);
        assert!(fc.can_send(25000));
        assert!(!fc.can_send(50001));
    }

    #[test]
    fn test_stream_flow_control_on_max_stream_data() {
        let mut fc = StreamFlowControl::new(0, 50000);
        fc.on_max_stream_data(100000);

        assert_eq!(fc.max_stream_data, 100000);
        assert!(fc.can_send(75000));
    }

    #[test]
    fn test_stream_flow_control_manager() {
        let mut mgr = StreamFlowControlManager::new(100000, 50000);

        let stream_fc = mgr.get_or_create(0);
        assert_eq!(stream_fc.stream_id(), 0);

        mgr.on_max_stream_data(0, 100000);
        let stream_fc = mgr.get(0).unwrap();
        assert_eq!(stream_fc.max_stream_data, 100000);
    }

    #[test]
    fn test_stream_auto_tune() {
        let mut fc = StreamFlowControl::new(0, 100000);

        fc.on_data_received(60000).unwrap();
        let new_limit = fc.should_advertise_limit();
        assert!(new_limit.is_some());
    }
}
