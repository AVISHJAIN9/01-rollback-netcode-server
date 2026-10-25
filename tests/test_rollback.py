import time

def run_rollback_accuracy_test():
    print("[*] Running Rollback Resimulation Accuracy Verification (TEST-ROL-01)...")
    from test_determinism import SimulationInstance
    
    # Run ground-truth linear simulation of 100 frames
    ground_truth = SimulationInstance(seed=999)
    inputs = [(i * 29 + 7) & 0x07 for i in range(100)]
    
    for inp in inputs:
        ground_truth.step(inp)
    truth_checksum = ground_truth.checksum()
    
    # Run speculative simulation where frame 80 had incorrect prediction, then rolled back
    speculative = SimulationInstance(seed=999)
    history = []
    
    # Run first 80 frames
    for i in range(80):
        speculative.step(inputs[i])
        history.append((speculative.pos_x, speculative.pos_y, speculative.vel_x, speculative.vel_y))
        
    # Speculate incorrectly on frames 80-90 (assume input = 0)
    for i in range(80, 90):
        speculative.step(0)
        
    # Now simulate rollback at frame 80: restore state from history[79], and apply true inputs 80..100
    snapshot_80 = history[79]
    speculative.frame = 80
    speculative.pos_x, speculative.pos_y, speculative.vel_x, speculative.vel_y = snapshot_80
    
    for i in range(80, 100):
        speculative.step(inputs[i])
        
    resim_checksum = speculative.checksum()
    print(f"[+] Ground Truth Checksum (Frame 100): {truth_checksum}")
    print(f"[+] Resimulated Checksum (Frame 100):  {resim_checksum}")
    
    if truth_checksum == resim_checksum:
        print("[SUCCESS] Rollback Resimulation is 100% identical to linear execution.")
        return True
    else:
        print("[FAIL] Resimulation checksum mismatch.")
        return False

if __name__ == "__main__":
    run_rollback_accuracy_test()
