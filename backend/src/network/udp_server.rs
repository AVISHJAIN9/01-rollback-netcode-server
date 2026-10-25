use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::protocol::packets::GamePacket;
use crate::protocol::version::VersionNegotiator;
use crate::simulation::rollback::RollbackEngine;

/// Session state for a connected peer
#[derive(Clone)]
pub struct PeerSession {
    pub player_id: u8,
    pub remote_addr: SocketAddr,
    pub engine: Arc<Mutex<RollbackEngine>>,
    pub last_ack_tick: u32,
}

/// Asynchronous UDP event loop for high-throughput packet handling.
pub struct UdpServer {
    bind_addr: SocketAddr,
    version_negotiator: VersionNegotiator,
    peers: Arc<Mutex<HashMap<SocketAddr, PeerSession>>>,
}

impl UdpServer {
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            version_negotiator: VersionNegotiator::default(),
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let socket = Arc::new(UdpSocket::bind(self.bind_addr).await?);
        tracing::info!("UDP server listening on {}", self.bind_addr);

        let mut buf = [0u8; 1500];
        let version_neg = self.version_negotiator;
        let peers = Arc::clone(&self.peers);
        let socket_clone = Arc::clone(&socket);

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, peer_addr)) => {
                    let packet_data = buf[..len].to_vec();
                    let socket_tx = Arc::clone(&socket_clone);
                    let peers_ref = Arc::clone(&peers);

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_packet(
                            peer_addr,
                            &packet_data,
                            version_neg,
                            socket_tx,
                            peers_ref,
                        )
                        .await
                        {
                            tracing::warn!("Packet handler error from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("UDP recv error: {}", e);
                    return Err(e.into());
                }
            }
        }
    }

    async fn handle_packet(
        peer_addr: SocketAddr,
        data: &[u8],
        version_neg: VersionNegotiator,
        socket: Arc<UdpSocket>,
        peers: Arc<Mutex<HashMap<SocketAddr, PeerSession>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Parse packet
        let packet = match GamePacket::decode(data) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to decode packet from {}: {}", peer_addr, e);
                // Send error packet
                let err_response = GamePacket::Error {
                    error_code: 400,
                    reason: "Malformed packet".to_string(),
                };
                let _ = socket.send_to(&err_response.encode(), peer_addr).await;
                return Ok(());
            }
        };

        let mut peers_guard = peers.lock().await;

        match packet {
            GamePacket::Handshake {
                client_version,
                capabilities: _,
                session_token: _,
            } => {
                // Validate version
                match version_neg.negotiate(client_version) {
                    Ok(negotiated_version) => {
                        let player_id = (peers_guard.len() % 2) as u8; // Simple player assignment
                        let assigned_token = [0u8; 16]; // Placeholder

                        let ack = GamePacket::HandshakeAck {
                            negotiated_version,
                            player_id,
                            tick_rate: 60,
                            assigned_token,
                        };

                        // Create new peer session
                        let session = PeerSession {
                            player_id,
                            remote_addr: peer_addr,
                            engine: Arc::new(Mutex::new(RollbackEngine::default())),
                            last_ack_tick: 0,
                        };

                        peers_guard.insert(peer_addr, session);
                        drop(peers_guard);

                        let _ = socket.send_to(&ack.encode(), peer_addr).await;
                        tracing::info!(
                            "Handshake complete with {} (player_id: {})",
                            peer_addr,
                            player_id
                        );
                    }
                    Err(e) => {
                        let err_pkt = GamePacket::Error {
                            error_code: 421,
                            reason: format!("{}", e),
                        };
                        let _ = socket.send_to(&err_pkt.encode(), peer_addr).await;
                        tracing::warn!("Version negotiation failed with {}: {}", peer_addr, e);
                    }
                }
            }

            GamePacket::InputFrame {
                player_id: remote_player_id,
                sequence_num: _,
                current_tick,
                input_bitmask,
                redundant_inputs: _,
            } => {
                // Find peer session
                if let Some(session) = peers_guard.get(&peer_addr) {
                    let mut engine = session.engine.lock().await;

                    // Inject input into engine
                    engine.handle_remote_input(remote_player_id as usize, current_tick, input_bitmask)
                        .ok();

                    // Send acknowledgment
                    let ack = GamePacket::StateAck {
                        player_id: session.player_id,
                        ack_tick: current_tick,
                        rtt_sample_ms: 0,
                    };
                    drop(engine);
                    drop(peers_guard);
                    let _ = socket.send_to(&ack.encode(), peer_addr).await;
                }
            }

            GamePacket::Ping {
                client_timestamp_ms,
            } => {
                let pong = GamePacket::Pong {
                    client_timestamp_ms,
                    server_timestamp_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                };
                drop(peers_guard);
                let _ = socket.send_to(&pong.encode(), peer_addr).await;
            }

            _ => {
                tracing::debug!("Unhandled packet type from {}: {:?}", peer_addr, packet);
            }
        }

        Ok(())
    }
}
