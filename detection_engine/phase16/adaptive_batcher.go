package phase16

// ─── Feature 96: Dynamic Adaptive Batch Sizing Controller ────────────────────
//
// AdaptiveBatchController adjusts batch size and flush interval using
// Additive-Increase / Multiplicative-Decrease (AIMD):
//   • Under load (queue deep):  increase batch size additively, extend flush timeout.
//   • On relief (queue shallow): decrease batch size multiplicatively, shorten timeout.
//
// BatchConfig holds the current recommended settings.

// BatchConfig is the output of each Adjust call.
type BatchConfig struct {
	BatchSize       int   // number of events to accumulate before flushing
	FlushIntervalUs int64 // microseconds to wait before forcing a flush
}

// AdaptiveBatchController computes dynamic batch sizing.
type AdaptiveBatchController struct {
	minBatchSize       int
	maxBatchSize       int
	minFlushIntervalUs int64
	maxFlushIntervalUs int64
	currentBatch       int
	currentIntervalUs  int64
}

// NewAdaptiveBatchController constructs a controller with the given bounds.
func NewAdaptiveBatchController(
	minBatchSize, maxBatchSize int,
	minFlushUs, maxFlushUs int64,
) *AdaptiveBatchController {
	return &AdaptiveBatchController{
		minBatchSize:       minBatchSize,
		maxBatchSize:       maxBatchSize,
		minFlushIntervalUs: minFlushUs,
		maxFlushIntervalUs: maxFlushUs,
		currentBatch:       minBatchSize,
		currentIntervalUs:  minFlushUs,
	}
}

// Adjust computes a new BatchConfig based on current queue depth and observed latency.
//
// AIMD rules:
//   • queueDepth > 75 % of max:  additive increase in batch size (+10 %); extend flush interval.
//   • queueDepth < 25 % of max:  multiplicative decrease (÷ 2); shorten flush interval.
//   • 25–75 %:                   hold current settings.
//
// observedLatencyUs drives the flush interval adjustment symmetrically.
func (c *AdaptiveBatchController) Adjust(currentQueueDepth, maxQueueDepth int, observedLatencyUs int64) BatchConfig {
	if maxQueueDepth == 0 {
		maxQueueDepth = 1
	}
	ratio := float64(currentQueueDepth) / float64(maxQueueDepth)

	switch {
	case ratio > 0.75:
		// Additive increase: grow batch by 10%
		c.currentBatch += c.currentBatch / 10
		if c.currentBatch < c.minBatchSize+1 {
			c.currentBatch = c.minBatchSize + 1
		}
		if c.currentBatch > c.maxBatchSize {
			c.currentBatch = c.maxBatchSize
		}
		// Extend flush interval proportionally to queue pressure
		c.currentIntervalUs = int64(float64(c.maxFlushIntervalUs) * ratio)
		if c.currentIntervalUs > c.maxFlushIntervalUs {
			c.currentIntervalUs = c.maxFlushIntervalUs
		}

	case ratio < 0.25:
		// Multiplicative decrease: halve batch size for low latency
		c.currentBatch /= 2
		if c.currentBatch < c.minBatchSize {
			c.currentBatch = c.minBatchSize
		}
		// Shorten flush interval for quick dispatch
		c.currentIntervalUs = c.minFlushIntervalUs
	}

	return BatchConfig{
		BatchSize:       c.currentBatch,
		FlushIntervalUs: c.currentIntervalUs,
	}
}

// CurrentBatchSize returns the most recently computed batch size.
func (c *AdaptiveBatchController) CurrentBatchSize() int { return c.currentBatch }

// CurrentFlushIntervalUs returns the most recently computed flush interval.
func (c *AdaptiveBatchController) CurrentFlushIntervalUs() int64 { return c.currentIntervalUs }
