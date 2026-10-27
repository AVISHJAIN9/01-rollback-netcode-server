# Systems Engineering Glossary: Rollback Netcode UDP Server

### Rollback Netcode
Networking architecture that predicts local state immediately and rewinds/resimulates upon receiving remote authorative inputs.

### Deterministic Lockstep
Simulation model where identical inputs on identical frames produce bit-for-bit identical world states.

### Ring Buffer
Circular fixed-size memory buffer storing past simulation states and input frames for constant-time rewind.

### Q16.16 Fixed-Point
Arithmetic representation replacing non-deterministic floating point math with integer math for cross-platform determinism.
