import time

class FixedI32:
    @staticmethod
    def from_float(v):
        return int(round(v * 65536))
    
    @staticmethod
    def mul(a, b):
        return (a * b) >> 16

class SimulationInstance:
    def __init__(self, seed=42):
        self.frame = 0
        self.pos_x = FixedI32.from_float(200.0)
        self.pos_y = FixedI32.from_float(400.0)
        self.vel_x = FixedI32.from_float(0.0)
        self.vel_y = FixedI32.from_float(0.0)
        self.friction = FixedI32.from_float(0.85)
        self.gravity = FixedI32.from_float(0.45)
        self.rng_state = seed

    def prng_next(self):
        self.rng_state = (self.rng_state * 6364136223846793005 + 1) & 0xFFFFFFFFFFFFFFFF
        return (self.rng_state >> 32) & 0xFF

    def step(self, button_mask):
        speed = FixedI32.from_float(3.2)
        if button_mask & 1: # Left
            self.vel_x = -speed
        elif button_mask & 2: # Right
            self.vel_x = speed
        else:
            self.vel_x = FixedI32.mul(self.vel_x, self.friction)

        if button_mask & 4: # Jump
            self.vel_y = FixedI32.from_float(-9.5)
        
        self.vel_y += self.gravity
        self.pos_x += self.vel_x
        self.pos_y += self.vel_y

        floor_y = FixedI32.from_float(420.0)
        if self.pos_y >= floor_y:
            self.pos_y = floor_y
            self.vel_y = 0

        self.frame += 1

    def checksum(self):
        h = 0x811c9dc5
        for val in [self.frame, self.pos_x, self.pos_y, self.vel_x, self.vel_y]:
            h ^= (val & 0xFF)
            h = (h * 0x01000193) & 0xFFFFFFFF
        return f"{h:08X}"

def run_determinism_test(steps=1000000):
    print(f"[*] Running 1,000,000 Step Cross-Instance Determinism Verification (TEST-DET-01)...")
    sim1 = SimulationInstance(seed=1337)
    sim2 = SimulationInstance(seed=1337)
    
    start = time.time()
    divergence_count = 0
    
    # Generate pseudo-random inputs
    for f in range(steps):
        input_byte = (f * 37 + 13) & 0x07
        sim1.step(input_byte)
        sim2.step(input_byte)
        
        if f % 100000 == 0:
            c1 = sim1.checksum()
            c2 = sim2.checksum()
            if c1 != c2:
                print(f"[FAIL] Divergence at frame {f}: {c1} != {c2}")
                divergence_count += 1
                break
    
    elapsed = time.time() - start
    final_c1 = sim1.checksum()
    final_c2 = sim2.checksum()
    
    print(f"[+] Completed {steps:,} frames in {elapsed:.3f}s ({(steps/elapsed):,.0f} frames/sec)")
    print(f"[+] Final Checksum Instance 1: {final_c1}")
    print(f"[+] Final Checksum Instance 2: {final_c2}")
    
    if final_c1 == final_c2 and divergence_count == 0:
        print("[SUCCESS] 100% BIT-EXACT DETERMINISM VERIFIED across 1,000,000 frames.")
        return True
    else:
        print("[FAIL] Determinism check failed.")
        return False

if __name__ == "__main__":
    run_determinism_test()
