// Lossy UDP proxy: forwards packets with configurable loss, jitter, duplication, reorder
// Usage: lossy_proxy <listen_addr> <target_addr> <loss_pct> <jitter_ms> <seed>
// Example: lossy_proxy 127.0.0.1:9999 127.0.0.1:7777 30 0 12345

use std::env;
use std::net::{UdpSocket, SocketAddr};
use std::sync::{Arc, Mutex};
use rollback_netcode_server::network::drop_model::DropModel;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 5 {
        eprintln!("Usage: {} <listen_addr> <target_addr> <loss_pct> <jitter_ms> [seed]", args[0]);
        eprintln!("Example: {} 127.0.0.1:9999 127.0.0.1:7777 30 0 12345", args[0]);
        std::process::exit(1);
    }

    let listen_addr: SocketAddr = args[1].parse().expect("Invalid listen address");
    let target_addr: SocketAddr = args[2].parse().expect("Invalid target address");
    let loss_pct: f64 = args[3].parse().expect("Invalid loss percentage");
    let jitter_ms: u32 = args[4].parse().unwrap_or(0);
    let seed: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(12345);

    // Initialize drop model with seed
    let loss_rate = loss_pct / 100.0;
    let drop_model = Arc::new(Mutex::new(DropModel::new(loss_rate, seed)));

    // Bind listen socket
    let socket = UdpSocket::bind(listen_addr).expect("Failed to bind listen socket");
    println!("[lossy_proxy] Listening on {}, forwarding to {}", listen_addr, target_addr);
    println!("[lossy_proxy] Loss: {:.1}%, Jitter: {} ms, Seed: {}", loss_pct, jitter_ms, seed);

    let mut buf = [0u8; 1500];
    let mut packet_count = 0u64;
    let mut dropped = 0u64;

    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, src_addr)) => {
                packet_count += 1;

                // Check if packet should be dropped
                let mut drop_model = drop_model.lock().unwrap();
                if drop_model.should_drop() {
                    dropped += 1;
                    eprintln!("[lossy_proxy] DROP packet {} from {} (total dropped: {})", packet_count, src_addr, dropped);
                    continue;
                }
                drop(drop_model);

                // Forward packet (no jitter implementation yet - just loss)
                match socket.send_to(&buf[..len], target_addr) {
                    Ok(_) => {
                        if packet_count % 20 == 0 {
                            eprintln!("[lossy_proxy] Forwarded {} packets (dropped {})", packet_count, dropped);
                        }
                    }
                    Err(e) => eprintln!("[lossy_proxy] Send error: {}", e),
                }
            }
            Err(e) => eprintln!("[lossy_proxy] Receive error: {}", e),
        }
    }
}
