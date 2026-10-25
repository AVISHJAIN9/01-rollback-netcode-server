# ADR 001: Dual Transport Layer (UDP Datagrams with WebSocket Fallback)

## Status
Accepted

## Context
Real-time rollback netcode requires ultra-low latency (< 16.66ms tick deadlines) and cannot tolerate Head-of-Line (HoL) blocking introduced by TCP retransmissions. However, browser environments and certain restrictive corporate NATs/firewalls restrict raw UDP datagrams without WebRTC data channels or WebSocket fallbacks.

## Decision
1. **Primary Transport (Native Clients / Dedicated Servers)**: Asynchronous non-blocking UDP with 5-frame redundant input bundling ($F, F-1, F-2, F-3, F-4$) to survive packet drops without retransmission stalls.
2. **Web Client Transport**: Binary WebSockets with framed datagram payloads and identical binary bitpacking to ensure cross-compatibility with the browser canvas runtime.
3. **Handshake & Versioning**: Versioned handshake payload enforcing protocol compatibility (`PROTOCOL_VERSION = 1`).

## Consequences
- Zero Head-of-Line blocking on native UDP pipes.
- Transparent fallback for browser clients.
- Redundant bundling increases packet payload by ~16 bytes, but eliminates the need for reliable ACK/NACK resend round-trips for transient losses.
