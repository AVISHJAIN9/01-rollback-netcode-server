# 04 - Algorithms and Mechanisms: Rollback Netcode Server

## 1. Fixed-Point Q16.16 Arithmetic [Specified]
### Why Chosen
Standard IEEE 754 floating-point operations (`f32`, `f64`) produce diverging results across different CPU architectures (x86 SSE/AVX vs ARM NEON vs WASM), compiler optimization flags (-O2 vs -O3), and microarchitectural FMA (Fused Multiply-Add) instructions. For a rollback engine, even a 1-bit drift in float representation causes immediate compounding desynchronization within 10 frames.

### Mechanism & Mathematical Formulation
We represent numbers as a signed 32-bit integer $X$, where the upper 16 bits represent the integer part and the lower 16 bits represent the fractional component:
$$\text{Value} = \frac{X}{2^{16}} = \frac{X}{65536}$$
- **Addition / Subtraction**: $A \pm B$ (Direct integer addition)
- **Multiplication**: $\lfloor (A \times B) \gg 16 \rfloor$ (Performed using intermediate 64-bit integer `i64` to prevent overflow)
- **Division**: $\lfloor (A \ll 16) / B \rfloor$ (64-bit shift before integer division)

### Complexity
- **Time Complexity**: $O(1)$ constant time (1-2 CPU cycles)
- **Space Complexity**: 4 bytes per scalar

### Alternatives Rejected
1. **IEEE 754 Floating Point with strict rounding modes**: Rejected because cross-platform compiler consistency (especially WebAssembly in browser clients) cannot be strictly guaranteed across all target hardware.
2. **Arbitrary Precision Rational Math (BigRational)**: Rejected due to severe heap allocation overhead and poor cache locality during 60 Hz tight loops.

---

## 2. Snapshot Circular Ring Buffer [Specified]
### Why Chosen
Rollback resimulation requires instant random access to any of the last $N=128$ frames. Dynamic allocation (e.g. `VecDeque` or linked lists) causes memory fragmentation, allocator locks, and cache misses.

### Data Structure Layout
```rust
pub struct RingBuffer<T, const CAP: usize> {
    buffer: [T; CAP],
    head_frame: u64,
}
```
Indexing a frame $F$ is computed via bitwise modulo:
$$\text{Index}(F) = F \pmod{128} = F \ \& \ 0x7F$$

### Complexity
- **Lookup Time**: $O(1)$ (1 clock cycle)
- **Insertion Time**: $O(1)$
- **Space Complexity**: $128 \times \text{sizeof}(T)$ (Statically allocated in cache)

---

## 3. Rollback Resimulation Engine [Specified]
### Mechanism
1. Client is at predicted local frame $F_{current} = 108$.
2. Remote input packet for frame $F_{remote} = 100$ arrives via UDP (8 frames late).
3. The engine verifies if $F_{remote} \ge (F_{current} - 128)$.
4. State is restored: $\text{ActiveState} \leftarrow \text{RingBuffer}[F_{remote}]$.
5. For $k = F_{remote}$ to $F_{current} - 1$:
   - Fetch confirmed inputs for frame $k$.
   - For missing remote inputs on frames $> F_{remote}$, apply input prediction (repeat last known input).
   - Execute deterministic physics step: $\text{ActiveState} \leftarrow \text{Step}(\text{ActiveState}, \text{Inputs}_k)$.
   - Store updated state: $\text{RingBuffer}[k+1] \leftarrow \text{ActiveState}$.
6. Local prediction continues forward from corrected $F_{current}$.

### Complexity
- **Time Complexity**: $O(\Delta F)$ where $\Delta F = F_{current} - F_{remote}$ (typically $1 \le \Delta F \le 10$).
- **Space Complexity**: $O(1)$ heap allocations.

---

## 4. NTP Clock Synchronization & Kalman Jitter Predictor [Specified]
### Mechanism
The client periodically transmits a 16-byte ping packet with client timestamp $T_1$. The server appends server receive timestamp $T_2$ and server transmit timestamp $T_3$. The client receives the response at $T_4$.
$$\text{RTT} = (T_4 - T_1) - (T_3 - T_2)$$
$$\text{Clock Skew } \theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}$$

The raw RTT series is fed into a 1D Kalman Filter to estimate the true underlying transit delay and variance $\sigma^2$, dynamically adjusting the local input delay window:
$$\text{InputDelay} = \left\lceil \frac{\hat{\text{RTT}}}{2 \times 16.666\text{ ms}} \right\rceil + \text{SafetyMargin}(\sigma)$$
