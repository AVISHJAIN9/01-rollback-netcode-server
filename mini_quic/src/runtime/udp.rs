// SPDX-License-Identifier: MIT OR Apache-2.0
// Mini-QUIC Transport Engine: Async UDP Socket Runtime & Buffer Pool
// Tokio-based event loop with batch send/recv and zero-copy recycled buffers

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;

/// Recycled UDP packet buffer.
#[derive(Clone, Debug)]
pub struct PacketBuffer {
    /// Data buffer (up to 1500 bytes typical).
    pub data: Vec<u8>,

    /// Current length of data.
    pub len: usize,

    /// Source address (populated on receive).
    pub src_addr: SocketAddr,
}

impl PacketBuffer {
    pub fn new(capacity: usize) -> Self {
        PacketBuffer {
            data: vec![0u8; capacity],
            len: 0,
            src_addr: "127.0.0.1:0".parse().unwrap(),
        }
    }

    /// Clear buffer for reuse.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Get mutable slice for reading into.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data[..]
    }

    /// Get slice of actual data.
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

/// Buffer pool for zero-allocation packet recycling.
#[derive(Clone)]
pub struct BufferPool {
    /// Pool of available buffers.
    buffers: Arc<RwLock<Vec<PacketBuffer>>>,

    /// Buffer size per packet.
    buffer_size: usize,

    /// Maximum buffers to keep in pool.
    max_buffers: usize,

    /// Telemetry: allocations.
    allocations: Arc<std::sync::atomic::AtomicU64>,

    /// Telemetry: deallocations.
    deallocations: Arc<std::sync::atomic::AtomicU64>,
}

impl BufferPool {
    /// Create a new buffer pool.
    pub fn new(buffer_size: usize, max_buffers: usize) -> Self {
        BufferPool {
            buffers: Arc::new(RwLock::new(Vec::with_capacity(max_buffers))),
            buffer_size,
            max_buffers,
            allocations: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            deallocations: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Get or allocate a buffer.
    pub async fn acquire(&self) -> PacketBuffer {
        let mut pool = self.buffers.write().await;

        if let Some(mut buf) = pool.pop() {
            buf.clear();
            buf
        } else {
            self.allocations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            PacketBuffer::new(self.buffer_size)
        }
    }

    /// Return buffer to pool.
    pub async fn release(&self, mut buffer: PacketBuffer) {
        buffer.clear();

        let mut pool = self.buffers.write().await;
        if pool.len() < self.max_buffers {
            pool.push(buffer);
        } else {
            self.deallocations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Get pool statistics.
    pub async fn stats(&self) -> (usize, u64, u64) {
        let pool = self.buffers.read().await;
        let available = pool.len();
        let allocs = self.allocations.load(std::sync::atomic::Ordering::Relaxed);
        let deallocs = self.deallocations.load(std::sync::atomic::Ordering::Relaxed);
        (available, allocs, deallocs)
    }
}

/// Async UDP packet handler trait for application logic.
#[async_trait::async_trait]
pub trait PacketHandler: Send + Sync {
    /// Handle an incoming packet.
    /// Returns true if packet was handled successfully.
    async fn handle_packet(&mut self, src_addr: SocketAddr, data: &[u8]) -> bool;

    /// Handle write readiness (for proactive packet transmission).
    async fn on_send_ready(&mut self) -> Vec<(SocketAddr, Vec<u8>)>;

    /// Get the local socket address.
    fn local_addr(&self) -> SocketAddr;
}

/// Async UDP event loop.
pub struct UdpEventLoop {
    socket: Arc<UdpSocket>,
    buffer_pool: BufferPool,
    mtu: usize,
}

impl UdpEventLoop {
    /// Create a new UDP event loop.
    pub async fn new(local_addr: SocketAddr, mtu: usize) -> Result<Self, std::io::Error> {
        let socket = UdpSocket::bind(local_addr).await?;

        Ok(UdpEventLoop {
            socket: Arc::new(socket),
            buffer_pool: BufferPool::new(mtu, 32),
            mtu,
        })
    }

    /// Run the event loop with the given packet handler.
    ///
    /// This method will run indefinitely, processing packets and calling the handler.
    pub async fn run<H: PacketHandler>(&self, handler: &mut H) -> Result<(), std::io::Error> {
        loop {
            // Acquire buffer from pool
            let mut buffer = self.buffer_pool.acquire().await;

            // Receive packet
            match self.socket.recv_from(buffer.as_mut_slice()).await {
                Ok((len, src_addr)) => {
                    buffer.len = len;
                    buffer.src_addr = src_addr;

                    // Handle packet
                    let _result = handler.handle_packet(src_addr, buffer.as_slice()).await;

                    // Return buffer to pool
                    self.buffer_pool.release(buffer).await;
                }
                Err(e) => {
                    return Err(e);
                }
            }

            // Check for outgoing packets
            let packets_to_send = handler.on_send_ready().await;
            for (dst_addr, data) in packets_to_send {
                let _bytes_sent = self.socket.send_to(&data, dst_addr).await?;
            }
        }
    }

    /// Send a packet to a destination.
    pub async fn send_packet(&self, dst_addr: SocketAddr, data: &[u8]) -> Result<usize, std::io::Error> {
        self.socket.send_to(data, dst_addr).await
    }

    /// Send multiple packets (batch send, simulated with individual sends on Tokio).
    pub async fn send_batch(&self, packets: &[(SocketAddr, Vec<u8>)]) -> Result<usize, std::io::Error> {
        let mut total = 0;
        for (dst_addr, data) in packets {
            match self.socket.send_to(data, dst_addr).await {
                Ok(n) => total += n,
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }

    /// Get local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, std::io::Error> {
        self.socket.local_addr()
    }

    /// Get buffer pool reference.
    pub fn buffer_pool(&self) -> &BufferPool {
        &self.buffer_pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_buffer_creation() {
        let buf = PacketBuffer::new(1500);
        assert_eq!(buf.data.capacity(), 1500);
        assert_eq!(buf.len, 0);
    }

    #[test]
    fn test_packet_buffer_slice() {
        let mut buf = PacketBuffer::new(1500);
        buf.len = 10;

        let slice = buf.as_slice();
        assert_eq!(slice.len(), 10);
    }

    #[tokio::test]
    async fn test_buffer_pool_acquire_release() {
        let pool = BufferPool::new(1500, 4);

        let buf1 = pool.acquire().await;
        assert_eq!(buf1.data.capacity(), 1500);

        pool.release(buf1).await;

        let (available, _allocs, _deallocs) = pool.stats().await;
        assert_eq!(available, 1);
    }

    #[tokio::test]
    async fn test_buffer_pool_stats() {
        let pool = BufferPool::new(1500, 4);

        let _buf1 = pool.acquire().await;
        let _buf2 = pool.acquire().await;

        let (available, allocs, _deallocs) = pool.stats().await;
        assert_eq!(available, 0);
        assert_eq!(allocs, 2);
    }

    #[tokio::test]
    async fn test_buffer_pool_max_size() {
        let pool = BufferPool::new(1500, 2);

        let buf1 = pool.acquire().await;
        let buf2 = pool.acquire().await;
        let buf3 = pool.acquire().await;

        pool.release(buf1).await;
        pool.release(buf2).await;
        pool.release(buf3).await;

        let (available, _allocs, deallocs) = pool.stats().await;
        assert_eq!(available, 2);
        assert_eq!(deallocs, 1);
    }
}
