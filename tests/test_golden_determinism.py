import time
from test_determinism import SimulationInstance

GOLDEN_CHECKSUMS = {
    1000: "D2E283B9",
    10000: "6EB013AE",
    100000: "3E7B3C88",
    1000000: "8A697184"
}

def run_golden_suite():
    print("==================================================================")
    print(" GATE G2: GOLDEN DETERMINISM & DESYNC RECOVERY VERIFICATION")
    print("==================================================================")
    
    sim = SimulationInstance(seed=1337)
    start = time.time()
    
    print("[*] Running Golden Input Simulation across milestone frames...")
    for f in range(1, 1000001):
        inp = (f * 37 + 13) & 0x07
        sim.step(inp)
        
        if f in GOLDEN_CHECKSUMS:
            actual = sim.checksum()
            expected = GOLDEN_CHECKSUMS[f]
            print(f"  - Frame {f:>7,}: Actual={actual} | Expected={expected} => {'MATCH' if actual == expected else 'MISMATCH'}")
            assert actual == expected, f"Golden mismatch at frame {f}"

    elapsed = time.time() - start
    print(f"[+] 1,000,000 frames executed in {elapsed:.3f}s ({(1000000/elapsed):,.0f} fps)")
    print("[SUCCESS] All golden checkpoint hashes matched bit-for-bit.")

    print("\n[*] Testing Desync Detection & Authoritative Snapshot Recovery...")
    client_sim = SimulationInstance(seed=1337)
    for f in range(1, 501):
        client_sim.step((f * 37 + 13) & 0x07)
    
    client_sim.pos_x += 1
    corrupted_hash = client_sim.checksum()
    
    server_sim = SimulationInstance(seed=1337)
    for f in range(1, 501):
        server_sim.step((f * 37 + 13) & 0x07)
    auth_hash = server_sim.checksum()
    
    print(f"  - Injected Client Checksum: {corrupted_hash}")
    print(f"  - Authoritative Checksum:   {auth_hash}")
    assert corrupted_hash != auth_hash, "Desync should be detected"
    print("  - [PASS] Desynchronization detected.")

    client_sim.pos_x = server_sim.pos_x
    client_sim.pos_y = server_sim.pos_y
    client_sim.vel_x = server_sim.vel_x
    client_sim.vel_y = server_sim.vel_y
    recovered_hash = client_sim.checksum()
    
    print(f"  - Recovered Client Checksum: {recovered_hash}")
    assert recovered_hash == auth_hash, "State recovery should restore checksum agreement"
    print("  - [PASS] Authoritative Snapshot Recovery restored 100% agreement.")

if __name__ == '__main__':
    run_golden_suite()
