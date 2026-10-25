package phase18

import (
	"fmt"
	"math"
	"sort"
	"sync"
	"sync/atomic"
	"time"
)

// ─── Feature 108: Stream Runtime Comprehensive Benchmark Harness ─────────────
//
// StreamRuntimeBenchmark drives synthetic event traffic through a configurable
// pipeline stage function and collects an HDR-style latency histogram.
// Supports three traffic patterns:
//   Uniform  – constant EPS
//   Zipfian  – heavy-tail key distribution (hot keys dominate)
//   Bursty   – alternates between quiet periods and high-EPS spike windows

// TrafficPattern selects the synthetic load profile.
type TrafficPattern int

const (
	PatternUniform  TrafficPattern = iota
	PatternZipfian
	PatternBursty
)

// BenchmarkConfig parameterises a benchmark run.
type BenchmarkConfig struct {
	TargetEPS      int
	DurationMs     int64
	Pattern        TrafficPattern
	NumProducers   int
	PipelineFn     func(event *CanonicalEvent) // the stage under test
}

// LatencyStats holds the computed percentile results from a benchmark run.
type LatencyStats struct {
	P50Ns   int64
	P90Ns   int64
	P99Ns   int64
	P999Ns  int64
	MeanNs  float64
	TotalOps int64
	ElapsedMs int64
	ActualEPS float64
}

func (s LatencyStats) String() string {
	return fmt.Sprintf("ops=%d EPS=%.0f P50=%dns P90=%dns P99=%dns P99.9=%dns mean=%.0fns",
		s.TotalOps, s.ActualEPS, s.P50Ns, s.P90Ns, s.P99Ns, s.P999Ns, s.MeanNs)
}

// StreamRuntimeBenchmark runs load tests and collects latency data.
type StreamRuntimeBenchmark struct {
	mu       sync.Mutex
	samples  []int64 // raw latency samples in ns
}

// NewStreamRuntimeBenchmark constructs a benchmark harness.
func NewStreamRuntimeBenchmark() *StreamRuntimeBenchmark {
	return &StreamRuntimeBenchmark{}
}

// Run executes the benchmark according to cfg and returns LatencyStats.
func (b *StreamRuntimeBenchmark) Run(cfg BenchmarkConfig) LatencyStats {
	b.mu.Lock()
	b.samples = b.samples[:0]
	b.mu.Unlock()

	deadline := time.Now().Add(time.Duration(cfg.DurationMs) * time.Millisecond)
	var ops atomic.Int64
	var wg sync.WaitGroup

	start := time.Now()

	for p := 0; p < cfg.NumProducers; p++ {
		p := p
		wg.Add(1)
		go func() {
			defer wg.Done()
			localSamples := make([]int64, 0, 10_000)

			for time.Now().Before(deadline) {
				event := &CanonicalEvent{
					EventID:     fmt.Sprintf("bench-p%d-e%d", p, ops.Load()),
					TimestampNs: time.Now().UnixNano(),
					EventType:   b.eventType(cfg.Pattern, int(ops.Load())),
				}

				t0 := time.Now()
				cfg.PipelineFn(event)
				latNs := time.Since(t0).Nanoseconds()

				localSamples = append(localSamples, latNs)
				ops.Add(1)

				// Throttle to approximate target EPS per producer
				epsPerProducer := cfg.TargetEPS / cfg.NumProducers
				if epsPerProducer > 0 {
					sleepNs := int64(1e9) / int64(epsPerProducer)
					if sleepNs > 100_000 { // only sleep if > 100µs
						time.Sleep(time.Duration(sleepNs))
					}
				}
			}

			b.mu.Lock()
			b.samples = append(b.samples, localSamples...)
			b.mu.Unlock()
		}()
	}

	wg.Wait()
	elapsed := time.Since(start)

	return b.computeStats(ops.Load(), elapsed)
}

func (b *StreamRuntimeBenchmark) eventType(p TrafficPattern, idx int) string {
	switch p {
	case PatternZipfian:
		// Zipfian approximation: skew heavily toward a few hot entity IDs
		bucket := zipfBucket(idx)
		return fmt.Sprintf("ProcessCreate_hot%d", bucket)
	case PatternBursty:
		if (idx/1000)%2 == 0 {
			return "NetworkConnect_burst"
		}
		return "Idle_trickle"
	default:
		return "ProcessCreate_uniform"
	}
}

// zipfBucket maps index to a Zipfian-distributed hot bucket (1 of ~10 buckets).
func zipfBucket(idx int) int {
	// Simple approximation: bucket = floor(log2(idx % 1000 + 1))
	x := idx%1000 + 1
	b := 0
	for x > 1 {
		x >>= 1
		b++
	}
	return b
}

func (b *StreamRuntimeBenchmark) computeStats(totalOps int64, elapsed time.Duration) LatencyStats {
	b.mu.Lock()
	samples := make([]int64, len(b.samples))
	copy(samples, b.samples)
	b.mu.Unlock()

	if len(samples) == 0 {
		return LatencyStats{TotalOps: totalOps, ElapsedMs: elapsed.Milliseconds()}
	}

	sort.Slice(samples, func(i, j int) bool { return samples[i] < samples[j] })

	var sum float64
	for _, s := range samples {
		sum += float64(s)
	}

	n := len(samples)
	perc := func(pct float64) int64 {
		idx := int(math.Ceil(pct/100.0*float64(n))) - 1
		if idx < 0 {
			idx = 0
		}
		if idx >= n {
			idx = n - 1
		}
		return samples[idx]
	}

	elapsedMs := elapsed.Milliseconds()
	actualEPS := 0.0
	if elapsed.Seconds() > 0 {
		actualEPS = float64(totalOps) / elapsed.Seconds()
	}

	return LatencyStats{
		P50Ns:     perc(50),
		P90Ns:     perc(90),
		P99Ns:     perc(99),
		P999Ns:    perc(99.9),
		MeanNs:    sum / float64(n),
		TotalOps:  totalOps,
		ElapsedMs: elapsedMs,
		ActualEPS: actualEPS,
	}
}
