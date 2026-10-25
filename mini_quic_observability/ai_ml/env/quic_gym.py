#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# Mini-QUIC Observability: Gymnasium RL Environment for Congestion Control
# State: [rtt, min_rtt, inflight, loss_rate, delivery_rate]
# Action: Pacing rate multiplier (0.5 to 1.5x)

import gymnasium as gym
import numpy as np
from gymnasium import spaces
from typing import Tuple, Dict, Any

class QuicCongestionEnv(gym.Env):
    """
    Gymnasium-compliant QUIC congestion control environment.
    
    State space (5 features):
    - RTT (current round-trip time in ms, normalized)
    - Min RTT (minimum RTT in ms, normalized)
    - Bytes in flight (normalized by congestion window)
    - Loss rate (packets lost / packets sent)
    - Delivery rate (bytes delivered / time interval)
    
    Action space:
    - Continuous: Pacing rate multiplier [0.5, 1.5]
    """
    
    metadata = {"render_modes": ["human"]}
    
    def __init__(self, config: Dict[str, Any] = None):
        super().__init__()
        
        self.config = config or {}
        self.mtu = self.config.get("mtu", 1200)
        self.initial_cwnd = self.config.get("initial_cwnd", self.mtu * 10)
        self.max_cwnd = self.config.get("max_cwnd", self.mtu * 100)
        self.min_cwnd = self.config.get("min_cwnd", self.mtu * 2)
        
        # Simulation parameters
        self.base_rtt_ms = self.config.get("base_rtt_ms", 50)
        self.jitter_ms = self.config.get("jitter_ms", 5)
        self.packet_loss_rate = self.config.get("packet_loss_rate", 0.01)
        self.max_steps = self.config.get("max_steps", 1000)
        
        # State variables
        self.cwnd_bytes = self.initial_cwnd
        self.min_rtt_ms = self.base_rtt_ms
        self.inflight_bytes = 0
        self.total_packets_sent = 0
        self.total_packets_lost = 0
        self.total_bytes_delivered = 0
        self.time_step = 0
        
        # Observation space: [rtt, min_rtt, inflight_ratio, loss_rate, delivery_rate]
        # All normalized to [0, 1]
        self.observation_space = spaces.Box(
            low=0.0, high=1.0, shape=(5,), dtype=np.float32
        )
        
        # Action space: Pacing rate multiplier [0.5, 1.5]
        self.action_space = spaces.Box(low=0.5, high=1.5, shape=(1,), dtype=np.float32)
        
        self.reward_cumulative = 0.0
        
    def _get_observation(self) -> np.ndarray:
        """Get current observation (normalized state)."""
        # Simulate RTT with jitter
        rtt_ms = self.base_rtt_ms + np.random.uniform(-self.jitter_ms, self.jitter_ms)
        
        # Normalize state
        rtt_normalized = np.clip(rtt_ms / (self.base_rtt_ms * 2), 0, 1)
        min_rtt_normalized = np.clip(self.min_rtt_ms / self.base_rtt_ms, 0, 1)
        inflight_ratio = np.clip(self.inflight_bytes / self.cwnd_bytes, 0, 1) if self.cwnd_bytes > 0 else 0
        loss_rate = np.clip(
            self.total_packets_lost / max(self.total_packets_sent, 1), 0, 1
        )
        delivery_rate = np.clip(
            self.total_bytes_delivered / max(self.cwnd_bytes, 1), 0, 1
        )
        
        return np.array([
            rtt_normalized,
            min_rtt_normalized,
            inflight_ratio,
            loss_rate,
            delivery_rate
        ], dtype=np.float32)
    
    def _calculate_reward(self) -> float:
        """Calculate reward based on throughput, latency, and loss."""
        # Reward components
        throughput_reward = (self.total_bytes_delivered / (self.time_step + 1)) / 1e6  # Mbps
        loss_penalty = -self.total_packets_lost * 0.1
        latency_penalty = -(self.min_rtt_ms / self.base_rtt_ms) * 5
        
        # Combined reward
        reward = throughput_reward + loss_penalty + latency_penalty
        return float(reward)
    
    def step(self, action: np.ndarray) -> Tuple[np.ndarray, float, bool, bool, Dict]:
        """Execute one step in the environment."""
        # Extract pacing rate multiplier from action
        pacing_multiplier = float(action[0])
        
        # Simulate packet transmission with pacing
        packets_to_send = max(1, int(self.cwnd_bytes / self.mtu * pacing_multiplier))
        
        # Simulate packet loss
        packets_lost = np.random.binomial(packets_to_send, self.packet_loss_rate)
        packets_delivered = packets_to_send - packets_lost
        
        # Update state
        self.total_packets_sent += packets_to_send
        self.total_packets_lost += packets_lost
        self.total_bytes_delivered += packets_delivered * self.mtu
        self.inflight_bytes = max(0, self.inflight_bytes - packets_delivered * self.mtu)
        
        # Update congestion window based on ACKs
        if packets_delivered > 0:
            # Simplified NewReno: cwnd increases by 1 MSS per RTT in slow start
            if self.cwnd_bytes < 32000:  # Simple SS threshold
                self.cwnd_bytes = min(self.cwnd_bytes + packets_delivered * self.mtu, self.max_cwnd)
            else:
                # Congestion avoidance: cwnd += MSS^2 / cwnd
                increment = (packets_delivered * self.mtu ** 2) / max(self.cwnd_bytes, 1)
                self.cwnd_bytes = min(self.cwnd_bytes + increment, self.max_cwnd)
        
        # Loss event: reduce window
        if packets_lost > 0:
            self.cwnd_bytes = max(self.min_cwnd, self.cwnd_bytes // 2)
        
        self.time_step += 1
        
        # Calculate reward
        reward = self._calculate_reward()
        self.reward_cumulative += reward
        
        # Check termination
        terminated = self.time_step >= self.max_steps
        truncated = False
        
        # Get observation
        observation = self._get_observation()
        
        # Info dict
        info = {
            "cwnd_bytes": self.cwnd_bytes,
            "inflight_bytes": self.inflight_bytes,
            "packets_sent": self.total_packets_sent,
            "packets_lost": self.total_packets_lost,
            "cumulative_reward": self.reward_cumulative,
        }
        
        return observation, reward, terminated, truncated, info
    
    def reset(self, seed: int = None, options: Dict = None) -> Tuple[np.ndarray, Dict]:
        """Reset environment to initial state."""
        super().reset(seed=seed)
        
        self.cwnd_bytes = self.initial_cwnd
        self.min_rtt_ms = self.base_rtt_ms
        self.inflight_bytes = 0
        self.total_packets_sent = 0
        self.total_packets_lost = 0
        self.total_bytes_delivered = 0
        self.time_step = 0
        self.reward_cumulative = 0.0
        
        observation = self._get_observation()
        info = {}
        
        return observation, info
    
    def render(self, mode: str = "human"):
        """Render environment state (for debugging)."""
        print(f"Step {self.time_step}: cwnd={self.cwnd_bytes}, "
              f"inflight={self.inflight_bytes}, "
              f"loss_rate={self.total_packets_lost/max(self.total_packets_sent, 1):.3f}, "
              f"reward={self.reward_cumulative:.2f}")


class QuicCongestionBenchmark:
    """Benchmark environment against multiple algorithms."""
    
    def __init__(self, num_episodes: int = 10, steps_per_episode: int = 500):
        self.num_episodes = num_episodes
        self.steps_per_episode = steps_per_episode
        self.env = QuicCongestionEnv({"max_steps": steps_per_episode})
    
    def run_newreno_baseline(self) -> Dict[str, float]:
        """Simulate NewReno performance."""
        rewards = []
        cwnd_values = []
        loss_rates = []
        
        for _ in range(self.num_episodes):
            obs, _ = self.env.reset()
            episode_reward = 0
            
            for _ in range(self.steps_per_episode):
                # NewReno action: maintain steady pacing (1.0x)
                action = np.array([1.0])
                obs, reward, terminated, truncated, info = self.env.step(action)
                episode_reward += reward
                cwnd_values.append(info["cwnd_bytes"])
                if info["packets_sent"] > 0:
                    loss_rates.append(info["packets_lost"] / info["packets_sent"])
                
                if terminated or truncated:
                    break
            
            rewards.append(episode_reward)
        
        return {
            "avg_reward": np.mean(rewards),
            "std_reward": np.std(rewards),
            "avg_cwnd": np.mean(cwnd_values),
            "avg_loss_rate": np.mean(loss_rates),
        }
    
    def run_cubic_baseline(self) -> Dict[str, float]:
        """Simulate CUBIC performance."""
        rewards = []
        cwnd_values = []
        loss_rates = []
        
        for _ in range(self.num_episodes):
            obs, _ = self.env.reset()
            episode_reward = 0
            
            for _ in range(self.steps_per_episode):
                # CUBIC action: aggressive pacing (1.2x)
                action = np.array([1.2])
                obs, reward, terminated, truncated, info = self.env.step(action)
                episode_reward += reward
                cwnd_values.append(info["cwnd_bytes"])
                if info["packets_sent"] > 0:
                    loss_rates.append(info["packets_lost"] / info["packets_sent"])
                
                if terminated or truncated:
                    break
            
            rewards.append(episode_reward)
        
        return {
            "avg_reward": np.mean(rewards),
            "std_reward": np.std(rewards),
            "avg_cwnd": np.mean(cwnd_values),
            "avg_loss_rate": np.mean(loss_rates),
        }


if __name__ == "__main__":
    # Test environment
    env = QuicCongestionEnv()
    obs, info = env.reset()
    
    print(f"Initial observation: {obs}")
    
    for _ in range(10):
        action = env.action_space.sample()
        obs, reward, terminated, truncated, info = env.step(action)
        print(f"Action: {action[0]:.2f}, Reward: {reward:.4f}, Info: {info}")
        
        if terminated or truncated:
            break
    
    # Run benchmarks
    print("\n--- Benchmarks ---")
    benchmark = QuicCongestionBenchmark(num_episodes=5, steps_per_episode=100)
    
    newreno_results = benchmark.run_newreno_baseline()
    print(f"NewReno: {newreno_results}")
    
    cubic_results = benchmark.run_cubic_baseline()
    print(f"CUBIC: {cubic_results}")
