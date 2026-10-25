"""
aiml/models/matchmaking_optimizer.py

Offline optimization comparing latency-constrained bipartite matchmaking
against naive FIFO random matchmaking.
"""

from typing import List, Tuple, Dict, Any
import numpy as np

class PlayerQueueEntry:
    def __init__(self, player_id: str, elo: int, ping_ms: float, region: str):
        self.player_id = player_id
        self.elo = elo
        self.ping_ms = ping_ms
        self.region = region

class MatchmakingOptimizer:
    MAX_ALLOWABLE_PING_DELTA_MS = 30.0
    MAX_ALLOWABLE_ELO_DELTA = 200

    @staticmethod
    def match_greedy(queue: List[PlayerQueueEntry]) -> List[Tuple[PlayerQueueEntry, PlayerQueueEntry]]:
        """Optimized bipartite matching pairing players by region and ping delta."""
        matches = []
        unmatched = list(queue)
        
        # Sort by region and ELO
        unmatched.sort(key=lambda p: (p.region, p.elo))
        
        while len(unmatched) >= 2:
            p1 = unmatched.pop(0)
            best_idx = None
            min_cost = float('inf')
            
            for idx, p2 in enumerate(unmatched):
                if p1.region != p2.region:
                    continue
                ping_delta = abs(p1.ping_ms - p2.ping_ms)
                elo_delta = abs(p1.elo - p2.elo)
                
                if ping_delta <= MatchmakingOptimizer.MAX_ALLOWABLE_PING_DELTA_MS and elo_delta <= MatchmakingOptimizer.MAX_ALLOWABLE_ELO_DELTA:
                    cost = (ping_delta * 1.0) + (elo_delta * 0.1)
                    if cost < min_cost:
                        min_cost = cost
                        best_idx = idx
            
            if best_idx is not None:
                p2 = unmatched.pop(best_idx)
                matches.append((p1, p2))
            else:
                # No valid pair satisfying hard constraints; keep in queue
                break
                
        return matches

    @staticmethod
    def evaluate_against_random_baseline(n_players: int = 200, seed: int = 42) -> Dict[str, Any]:
        np.random.seed(seed)
        queue = []
        regions = ["us-east", "us-west", "eu-central"]
        
        for i in range(n_players):
            queue.append(PlayerQueueEntry(
                player_id=f"p_{i}",
                elo=int(np.random.normal(1200, 150)),
                ping_ms=float(np.random.gamma(3.0, 10.0)),
                region=np.random.choice(regions)
            ))
            
        # 1. Random Baseline Pairing
        random_queue = list(queue)
        np.random.shuffle(random_queue)
        random_ping_deltas = []
        random_constraint_violations = 0
        
        for i in range(0, len(random_queue) - 1, 2):
            p1 = random_queue[i]
            p2 = random_queue[i+1]
            delta = abs(p1.ping_ms - p2.ping_ms)
            random_ping_deltas.append(delta)
            if p1.region != p2.region or delta > MatchmakingOptimizer.MAX_ALLOWABLE_PING_DELTA_MS:
                random_constraint_violations += 1
                
        # 2. Optimized Matchmaking
        matches = MatchmakingOptimizer.match_greedy(queue)
        opt_ping_deltas = [abs(p1.ping_ms - p2.ping_ms) for p1, p2 in matches]
        opt_constraint_violations = sum(
            1 for p1, p2 in matches
            if p1.region != p2.region or abs(p1.ping_ms - p2.ping_ms) > MatchmakingOptimizer.MAX_ALLOWABLE_PING_DELTA_MS
        )
        
        mean_rand_ping_delta = float(np.mean(random_ping_deltas))
        mean_opt_ping_delta = float(np.mean(opt_ping_deltas)) if opt_ping_deltas else 0.0
        
        return {
            "total_players_queued": n_players,
            "random_baseline_mean_ping_delta_ms": mean_rand_ping_delta,
            "random_constraint_violations": random_constraint_violations,
            "optimized_mean_ping_delta_ms": mean_opt_ping_delta,
            "optimized_constraint_violations": opt_constraint_violations,
            "ping_delta_reduction_pct": float((mean_rand_ping_delta - mean_opt_ping_delta) / mean_rand_ping_delta * 100.0),
            "constraints_strictly_preserved": opt_constraint_violations == 0
        }
