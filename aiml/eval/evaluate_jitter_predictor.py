import numpy as np

class KalmanFilter1D:
    def __init__(self, q=0.05, r=16.0):
        self.x = 40.0 # Initial RTT estimate (ms)
        self.p = 1.0
        self.q = q
        self.r = r

    def update(self, z):
        p_pred = self.p + self.q
        k = p_pred / (p_pred + self.r)
        self.x = self.x + k * (z - self.x)
        self.p = (1.0 - k) * p_pred
        return self.x

def evaluate_jitter_predictor():
    print("==================================================================")
    print(" AIML TIER A: ADAPTIVE JITTER & INPUT DELAY PREDICTOR EVALUATION")
    print("==================================================================")
    np.random.seed(1337)
    
    n_steps = 2000
    base_rtt = 45.0 + np.sin(np.linspace(0, 20, n_steps)) * 10.0
    jitter = np.random.normal(0, 4.0, n_steps)
    spikes = (np.random.rand(n_steps) > 0.96) * np.random.uniform(40, 90, n_steps)
    true_rtt = base_rtt + jitter + spikes
    
    # 1. Baseline: EWMA (alpha = 0.2)
    ewma_preds = []
    curr_ewma = true_rtt[0]
    for z in true_rtt:
        curr_ewma = 0.2 * z + 0.8 * curr_ewma
        ewma_preds.append(curr_ewma)
    ewma_preds = np.array(ewma_preds)
    
    # 2. Kalman Filter
    kf = KalmanFilter1D(q=0.05, r=16.0)
    kf_preds = np.array([kf.update(z) for z in true_rtt])
    
    ideal_smoothed = base_rtt
    ewma_frame_mae = np.mean(np.abs(ewma_preds - ideal_smoothed)) / 16.666
    kf_frame_mae = np.mean(np.abs(kf_preds - ideal_smoothed)) / 16.666
    
    print(f"[BASELINE] EWMA (alpha=0.2) Mean Absolute Error: {ewma_frame_mae:.4f} frames")
    print(f"[MODEL]    Adaptive Kalman Filter Error:        {kf_frame_mae:.4f} frames (Target < 0.35)")
    
    if kf_frame_mae < 0.35:
        print("\n[VERIFICATION PASS] Kalman Predictor achieved < 0.35 frames estimation error.")
    else:
        print("\n[VERIFICATION FAIL] Kalman Predictor exceeded error bound.")

if __name__ == '__main__':
    evaluate_jitter_predictor()
