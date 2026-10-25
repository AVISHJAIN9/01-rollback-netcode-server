// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Stream Reassembly Buffer (RFC 9000 §3)
// Out-of-order receive buffer with sparse interval tracking and contiguous reader

use std::collections::BTreeMap;

/// Sparse interval in stream reassembly buffer.
/// Represents a range of bytes [offset, offset + len) that have been received.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Interval {
    offset: u64,
    len: u64,
}

impl Interval {
    fn new(offset: u64, len: u64) -> Self {
        Interval { offset, len }
    }

    fn end(&self) -> u64 {
        self.offset + self.len
    }

    fn overlaps(&self, other: &Interval) -> bool {
        self.offset < other.end() && other.offset < self.end()
    }

    fn adjacent(&self, other: &Interval) -> bool {
        self.end() == other.offset || other.end() == self.offset
    }

    fn merge(&mut self, other: &Interval) {
        let new_offset = self.offset.min(other.offset);
        let new_end = self.end().max(other.end());
        self.offset = new_offset;
        self.len = new_end - new_offset;
    }
}

/// Stream receive buffer: out-of-order reassembly with sparse interval tracking.
#[derive(Clone, Debug)]
pub struct StreamReceiveBuffer {
    /// Buffer data storage (indexed by offset in stream).
    data: Vec<u8>,

    /// Sparse intervals of received data (offset -> interval length).
    received: BTreeMap<u64, u64>,

    /// Current read offset (contiguous data up to this point has been read).
    read_offset: u64,

    /// Next expected offset (for new data ingestion).
    write_offset: u64,

    /// Maximum buffer size (for backpressure).
    max_size: usize,

    /// Total bytes received (for telemetry).
    total_bytes_received: u64,

    /// Flag: stream has been reset or closed.
    is_closed: bool,

    /// Offset at which stream was finalized (FIN bit received).
    final_offset: Option<u64>,
}

impl StreamReceiveBuffer {
    /// Create a new receive buffer.
    pub fn new(max_size: usize) -> Self {
        StreamReceiveBuffer {
            data: Vec::with_capacity(max_size),
            received: BTreeMap::new(),
            read_offset: 0,
            write_offset: 0,
            max_size,
            total_bytes_received: 0,
            is_closed: false,
            final_offset: None,
        }
    }

    /// Insert received data at the given offset.
    ///
    /// Returns `true` if data was inserted, `false` if invalid or duplicate.
    pub fn insert(&mut self, offset: u64, data: &[u8]) -> Result<bool, BufferError> {
        // Check if stream is closed
        if self.is_closed {
            return Err(BufferError::StreamClosed);
        }

        // Check if we have a FIN offset and this data is beyond it
        if let Some(fin_offset) = self.final_offset {
            if offset + data.len() as u64 > fin_offset {
                return Err(BufferError::DataBeyondFin);
            }
        }

        if data.is_empty() {
            return Ok(false);
        }

        // Ensure buffer won't exceed max size
        let end_offset = offset + data.len() as u64;
        if end_offset as usize > self.max_size {
            return Err(BufferError::BufferFull);
        }

        // Expand data buffer if needed
        if end_offset as usize > self.data.len() {
            self.data.resize(end_offset as usize, 0);
        }

        // Copy data into buffer
        let start_idx = offset as usize;
        let end_idx = end_offset as usize;
        self.data[start_idx..end_idx].copy_from_slice(data);

        // Update received intervals
        self.merge_received_interval(offset, data.len() as u64);

        self.total_bytes_received += data.len() as u64;
        self.write_offset = self.write_offset.max(end_offset);

        Ok(true)
    }

    /// Merge a new interval into the received set.
    fn merge_received_interval(&mut self, offset: u64, len: u64) {
        let new_interval = Interval::new(offset, len);

        let mut merged_interval = new_interval.clone();

        // Find and merge overlapping/adjacent intervals
        let mut to_remove = Vec::new();
        for (stored_offset, stored_len) in self.received.iter() {
            let stored = Interval::new(*stored_offset, *stored_len);
            if merged_interval.overlaps(&stored) || merged_interval.adjacent(&stored) {
                merged_interval.merge(&stored);
                to_remove.push(*stored_offset);
            }
        }

        // Remove merged intervals
        for offset in to_remove {
            self.received.remove(&offset);
        }

        // Insert merged interval
        self.received.insert(merged_interval.offset, merged_interval.len);
    }

    /// Read contiguous data from read_offset.
    ///
    /// Returns the data and advances read_offset.
    pub fn read_contiguous(&mut self) -> Vec<u8> {
        let read_interval = self
            .received
            .range(..=self.read_offset)
            .next_back()
            .and_then(|(offset, len)| {
                let interval = Interval::new(*offset, *len);
                if interval.offset <= self.read_offset && self.read_offset < interval.end() {
                    Some(interval)
                } else {
                    None
                }
            });

        let mut result = Vec::new();

        if let Some(interval) = read_interval {
            let available_len = interval.end() - self.read_offset;
            let start_idx = self.read_offset as usize;
            let end_idx = (self.read_offset + available_len) as usize;

            result.extend_from_slice(&self.data[start_idx..end_idx]);
            self.read_offset += available_len;
        }

        result
    }

    /// Get the number of contiguous bytes available to read.
    pub fn contiguous_len(&self) -> u64 {
        self.received
            .range(..=self.read_offset)
            .next_back()
            .and_then(|(offset, len)| {
                let interval = Interval::new(*offset, *len);
                if interval.offset <= self.read_offset && self.read_offset < interval.end() {
                    Some(interval.end() - self.read_offset)
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    /// Mark stream as finalized at the given offset (FIN bit received).
    ///
    /// Returns `true` if finalization is valid, `false` if conflicting.
    pub fn finalize(&mut self, final_offset: u64) -> Result<(), BufferError> {
        if let Some(existing) = self.final_offset {
            if existing != final_offset {
                return Err(BufferError::FinalOffsetMismatch);
            }
        } else {
            self.final_offset = Some(final_offset);
        }
        Ok(())
    }

    /// Check if stream is complete (all data up to FIN has been received).
    pub fn is_complete(&self) -> bool {
        if let Some(final_offset) = self.final_offset {
            self.read_offset >= final_offset
        } else {
            false
        }
    }

    /// Close the buffer (reject further inserts).
    pub fn close(&mut self) {
        self.is_closed = true;
    }

    /// Get read offset (bytes already consumed).
    pub fn read_offset(&self) -> u64 {
        self.read_offset
    }

    /// Get total bytes received (unread + read).
    pub fn total_received(&self) -> u64 {
        self.total_bytes_received
    }

    /// Get number of bytes buffered (not yet read).
    pub fn buffered_len(&self) -> u64 {
        self.write_offset - self.read_offset
    }

    /// Get final offset if stream was finalized.
    pub fn final_offset(&self) -> Option<u64> {
        self.final_offset
    }

    /// Check if buffer is closed.
    pub fn is_closed(&self) -> bool {
        self.is_closed
    }
}

/// Stream send buffer: chunked transmission with flow control.
#[derive(Clone, Debug)]
pub struct StreamSendBuffer {
    /// Buffered data waiting to be sent.
    data: Vec<u8>,

    /// Current write offset for new data.
    write_offset: u64,

    /// Offset of next byte to send.
    send_offset: u64,

    /// Offset of next byte to acknowledge.
    ack_offset: u64,

    /// Maximum buffer size.
    max_size: usize,

    /// Stream has been finalized (FIN will be sent after write_offset).
    is_finalized: bool,

    /// Telemetry: bytes sent.
    bytes_sent: u64,

    /// Telemetry: bytes acknowledged.
    bytes_acked: u64,
}

impl StreamSendBuffer {
    pub fn new(max_size: usize) -> Self {
        StreamSendBuffer {
            data: Vec::with_capacity(max_size),
            write_offset: 0,
            send_offset: 0,
            ack_offset: 0,
            max_size,
            is_finalized: false,
            bytes_sent: 0,
            bytes_acked: 0,
        }
    }

    /// Write data to the send buffer.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, BufferError> {
        if self.is_finalized {
            return Err(BufferError::StreamFinalized);
        }

        if data.is_empty() {
            return Ok(0);
        }

        let space_available = self.max_size - self.data.len();
        let to_write = data.len().min(space_available);

        if to_write == 0 {
            return Err(BufferError::BufferFull);
        }

        self.data.extend_from_slice(&data[..to_write]);
        self.write_offset += to_write as u64;

        Ok(to_write)
    }

    /// Get data to send (from send_offset to write_offset).
    pub fn get_sendable(&self, max_len: usize) -> (u64, Vec<u8>) {
        let available = (self.write_offset - self.send_offset) as usize;
        let to_send = available.min(max_len);

        if to_send == 0 {
            return (self.send_offset, Vec::new());
        }

        let start_idx = (self.send_offset - self.ack_offset) as usize;
        let end_idx = start_idx + to_send;

        let data = self.data[start_idx..end_idx].to_vec();
        (self.send_offset, data)
    }

    /// Mark bytes as sent.
    pub fn mark_sent(&mut self, offset: u64, len: u64) {
        self.send_offset = (offset + len).max(self.send_offset);
        self.bytes_sent += len;
    }

    /// Mark bytes as acknowledged (remove from buffer).
    pub fn mark_acked(&mut self, offset: u64, len: u64) {
        let ack_end = offset + len;
        if ack_end > self.ack_offset {
            let newly_acked = (ack_end - self.ack_offset) as usize;
            self.ack_offset = ack_end;
            self.bytes_acked += newly_acked as u64;

            // Compact buffer: remove acknowledged data
            if self.ack_offset > 0 {
                let keep_start = (self.ack_offset - (self.ack_offset.min(self.ack_offset))) as usize;
                self.data.drain(..keep_start);
            }
        }
    }

    /// Finalize the stream (mark FIN flag).
    pub fn finalize(&mut self) -> Result<(), BufferError> {
        if self.is_finalized {
            return Err(BufferError::StreamFinalized);
        }
        self.is_finalized = true;
        Ok(())
    }

    /// Get bytes buffered (not yet acknowledged).
    pub fn buffered_len(&self) -> u64 {
        (self.write_offset - self.ack_offset).max(0)
    }

    /// Get bytes in flight (sent but not acknowledged).
    pub fn bytes_in_flight(&self) -> u64 {
        (self.send_offset - self.ack_offset).max(0)
    }

    /// Check if stream is finalized.
    pub fn is_finalized(&self) -> bool {
        self.is_finalized
    }

    /// Get bytes acknowledged so far.
    pub fn bytes_acked(&self) -> u64 {
        self.bytes_acked
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BufferError {
    BufferFull,
    StreamClosed,
    StreamFinalized,
    DataBeyondFin,
    FinalOffsetMismatch,
}

impl std::fmt::Display for BufferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferFull => write!(f, "buffer full"),
            Self::StreamClosed => write!(f, "stream closed"),
            Self::StreamFinalized => write!(f, "stream finalized"),
            Self::DataBeyondFin => write!(f, "data beyond FIN offset"),
            Self::FinalOffsetMismatch => write!(f, "FIN offset mismatch"),
        }
    }
}

impl std::error::Error for BufferError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_receive_buffer_init() {
        let buf = StreamReceiveBuffer::new(10000);
        assert_eq!(buf.read_offset(), 0);
        assert_eq!(buf.total_received(), 0);
        assert!(!buf.is_closed());
    }

    #[test]
    fn test_receive_buffer_insert_contiguous() {
        let mut buf = StreamReceiveBuffer::new(10000);

        let inserted = buf.insert(0, b"hello").unwrap();
        assert!(inserted);
        assert_eq!(buf.total_received(), 5);

        let data = buf.read_contiguous();
        assert_eq!(data, b"hello");
        assert_eq!(buf.read_offset(), 5);
    }

    #[test]
    fn test_receive_buffer_insert_ooo() {
        let mut buf = StreamReceiveBuffer::new(10000);

        // Insert out of order
        buf.insert(5, b"world").unwrap();
        assert_eq!(buf.contiguous_len(), 0); // Can't read anything yet

        buf.insert(0, b"hello").unwrap();
        let data = buf.read_contiguous();
        assert_eq!(data, b"helloworld");
    }

    #[test]
    fn test_receive_buffer_finalize() {
        let mut buf = StreamReceiveBuffer::new(10000);

        buf.insert(0, b"hello").unwrap();
        buf.finalize(5).unwrap();

        assert_eq!(buf.final_offset(), Some(5));
        assert!(buf.is_complete());
    }

    #[test]
    fn test_receive_buffer_close() {
        let mut buf = StreamReceiveBuffer::new(10000);

        buf.close();
        assert!(buf.is_closed());

        let result = buf.insert(0, b"hello");
        assert_eq!(result, Err(BufferError::StreamClosed));
    }

    #[test]
    fn test_send_buffer_write() {
        let mut buf = StreamSendBuffer::new(1000);

        let written = buf.write(b"hello").unwrap();
        assert_eq!(written, 5);
        assert_eq!(buf.buffered_len(), 5);
    }

    #[test]
    fn test_send_buffer_mark_sent() {
        let mut buf = StreamSendBuffer::new(1000);

        buf.write(b"hello").unwrap();
        buf.mark_sent(0, 5);

        assert_eq!(buf.bytes_in_flight(), 5);
    }

    #[test]
    fn test_send_buffer_mark_acked() {
        let mut buf = StreamSendBuffer::new(1000);

        buf.write(b"hello").unwrap();
        buf.mark_sent(0, 5);
        buf.mark_acked(0, 5);

        assert_eq!(buf.bytes_in_flight(), 0);
        assert_eq!(buf.bytes_acked(), 5);
    }

    #[test]
    fn test_send_buffer_finalize() {
        let mut buf = StreamSendBuffer::new(1000);

        buf.write(b"hello").unwrap();
        buf.finalize().unwrap();

        assert!(buf.is_finalized());

        let result = buf.write(b"world");
        assert_eq!(result, Err(BufferError::StreamFinalized));
    }
}
