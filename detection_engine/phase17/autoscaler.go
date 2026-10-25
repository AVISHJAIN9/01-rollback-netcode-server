package phase17

import (
	"sync"
	"sync/atomic"
	"time"
)

// ─── Feature 98: Adaptive Worker Thread-Pool Autoscaler ──────────────────────
//
// AdaptiveThreadPool monitors queue occupancy via an EWMA of utilisation.
// It scales up when utilisation > 75% for > scaleUpAfter and scales down
// after a cooldown period once utilisation drops.

// WorkerFunc is the function executed by each worker goroutine.
type WorkerFunc func()

// AdaptiveThreadPool manages a dynamic pool of goroutines.
type AdaptiveThreadPool struct {
	mu          sync.Mutex
	minWorkers  int
	maxWorkers  int
	current     int
	ewmaUtil    float64   // exponential weighted moving average of utilisation
	ewmaAlpha   float64   // smoothing factor
	lastScaleUp time.Time // for cooldown tracking
	cooldown    time.Duration
	cancelFuncs []func() // per-worker cancellation hooks

	// Telemetry
	totalSpawned  atomic.Int64
	totalRetired  atomic.Int64
}

// NewAdaptiveThreadPool constructs a pool starting at minWorkers.
func NewAdaptiveThreadPool(minWorkers, maxWorkers int) *AdaptiveThreadPool {
	p := &AdaptiveThreadPool{
		minWorkers: minWorkers,
		maxWorkers: maxWorkers,
		current:    minWorkers,
		ewmaAlpha:  0.3,
		cooldown:   30 * time.Second,
	}
	p.cancelFuncs = make([]func(), 0, maxWorkers)
	return p
}

// CurrentWorkers returns the current pool size.
func (p *AdaptiveThreadPool) CurrentWorkers() int {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.current
}

// Observe records a new queue utilisation sample (0–1) and adjusts the EWMA.
// Call this periodically (e.g., every 100ms) to feed the autoscaler.
func (p *AdaptiveThreadPool) Observe(utilisation float64) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.ewmaUtil = p.ewmaAlpha*utilisation + (1-p.ewmaAlpha)*p.ewmaUtil
}

// EWMAUtilisation returns the current smoothed utilisation in [0,1].
func (p *AdaptiveThreadPool) EWMAUtilisation() float64 {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.ewmaUtil
}

// ScaleUp spawns one additional worker goroutine (up to maxWorkers).
// onWork is called in the new goroutine and must block until stopped.
func (p *AdaptiveThreadPool) ScaleUp(onWork func(stop <-chan struct{})) bool {
	p.mu.Lock()
	defer p.mu.Unlock()

	if p.current >= p.maxWorkers {
		return false
	}

	stopCh := make(chan struct{})
	p.cancelFuncs = append(p.cancelFuncs, func() { close(stopCh) })
	p.current++
	p.lastScaleUp = time.Now()
	p.totalSpawned.Add(1)

	go onWork(stopCh)
	return true
}

// ScaleDown removes one worker (down to minWorkers) by closing its stop channel.
func (p *AdaptiveThreadPool) ScaleDown() bool {
	p.mu.Lock()
	defer p.mu.Unlock()

	if p.current <= p.minWorkers {
		return false
	}
	if time.Since(p.lastScaleUp) < p.cooldown {
		return false // within cooldown window
	}

	// Signal the most-recently-added worker to stop.
	last := len(p.cancelFuncs) - 1
	if last >= 0 {
		p.cancelFuncs[last]()
		p.cancelFuncs = p.cancelFuncs[:last]
	}
	p.current--
	p.totalRetired.Add(1)
	return true
}

// AutoAdjust evaluates the current EWMA and calls ScaleUp or ScaleDown.
// Returns the number of workers added (positive) or removed (negative).
func (p *AdaptiveThreadPool) AutoAdjust(utilisation float64, onWork func(stop <-chan struct{})) int {
	p.Observe(utilisation)

	p.mu.Lock()
	ewma := p.ewmaUtil
	p.mu.Unlock()

	switch {
	case ewma > 0.75:
		if p.ScaleUp(onWork) {
			return 1
		}
	case ewma < 0.30:
		if p.ScaleDown() {
			return -1
		}
	}
	return 0
}

// ForceScaleTo forcibly sets the worker count to target (used in tests).
// Workers between current and target are either spawned or cancelled.
func (p *AdaptiveThreadPool) ForceScaleTo(target int, onWork func(stop <-chan struct{})) {
	p.mu.Lock()
	defer p.mu.Unlock()

	for p.current < target && p.current < p.maxWorkers {
		stopCh := make(chan struct{})
		p.cancelFuncs = append(p.cancelFuncs, func() { close(stopCh) })
		p.current++
		p.totalSpawned.Add(1)
		go onWork(stopCh)
	}
	for p.current > target && p.current > p.minWorkers {
		last := len(p.cancelFuncs) - 1
		if last >= 0 {
			p.cancelFuncs[last]()
			p.cancelFuncs = p.cancelFuncs[:last]
		}
		p.current--
		p.totalRetired.Add(1)
	}
}

// StopAll shuts down all worker goroutines.
func (p *AdaptiveThreadPool) StopAll() {
	p.mu.Lock()
	defer p.mu.Unlock()
	for _, cancel := range p.cancelFuncs {
		cancel()
	}
	p.cancelFuncs = p.cancelFuncs[:0]
	p.current = p.minWorkers
}
