# 06 - API & Protocol Reference: Rollback Netcode Server

## 1. UDP Binary Datagram Protocol [Specified]

### 1.1 Client Input Packet Layout (UDP)
| Offset (Bytes) | Field Name | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x00 - 0x03` | `session_token` | `u32` | Ephemeral match token |
| `0x04 - 0x07` | `player_id` | `u32` | Player index in match (0 or 1) |
| `0x08 - 0x0B` | `target_frame` | `u32` | Frame number for this input |
| `0x0C - 0x0D` | `input_bitmask` | `u16` | Digital buttons (Left, Right, Jump, Attack) |
| `0x0E - 0x11` | `analog_dx_q16` | `i32` | Q16.16 horizontal stick delta |
| `0x12 - 0x15` | `analog_dy_q16` | `i32` | Q16.16 vertical stick delta |
| `0x16 - 0x1D` | `history_inputs` | `u64` | Packed bitmasks for frames $F-1, F-2, F-3, F-4$ |
| `0x1E - 0x25` | `state_checksum` | `u64` | Blake3 64-bit hash of state at frame $F-1$ |

---

## 2. REST & WebSocket Endpoints [Specified]

### 2.1 Matchmaking Queue Ticket
- **URL**: `POST /api/v1/match/queue`
- **Request Headers**: `Authorization: Bearer <JWT>`
- **Request Body**:
```json
{
  "player_id": "usr_99812a",
  "mmr": 1450,
  "preferred_region": "us-east",
  "client_version": "1.0.0"
}
```
- **Response Body (200 OK)**:
```json
{
  "ticket_id": "tkt_01h8a9bc",
  "status": "QUEUED",
  "estimated_wait_sec": 4.2
}
```

### 2.2 LLM Desync Explainer
- **URL**: `POST /api/v1/ai/explain-desync`
- **Request Body**:
```json
{
  "session_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
  "frame_index": 442,
  "server_state": {
    "entities": [
      {"id": 1, "pos_x": 102450, "pos_y": 65536, "vel_x": 3200, "vel_y": 0}
    ]
  },
  "client_state": {
    "entities": [
      {"id": 1, "pos_x": 102450, "pos_y": 65536, "vel_x": 3199, "vel_y": 0}
    ]
  }
}
```
- **Response Body (200 OK)**:
```json
{
  "root_cause": "Fixed-point precision divergence in horizontal velocity friction damping on Entity 1.",
  "affected_entities": [1],
  "diverging_fields": ["vel_x"],
  "severity": "CRITICAL",
  "remediation": "Check for unrounded bitshift in friction multiplier calculation."
}
```
