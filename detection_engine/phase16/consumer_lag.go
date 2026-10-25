package phase16

import (
	"fmt"
	"sync"
	"time"
)

// ─── Feature 93: Consumer Lag Monitoring & Velocity Estimation ───────────────
//
// ConsumerLagMonitor tracks per-partition Kafka-style offsets and computes:
//   • Instantaneous lag: LogEndOffset - CurrentOffset.
//   • Velocity: dLag/dt = ProductionRate - ConsumptionRate (events per second).
//   • Projected breach: time until lag crosses a configured SLA threshold.

// PartitionLagReport contains the lag snapshot for one partition.
type PartitionLagReport struct {
	PartitionID      int
	LogEndOffset     uint64
	CurrentOffset    uint64
	LagEvents        uint64
	ProductionEPS    float64 // events/sec entering the topic
	ConsumptionEPS   float64 // events/sec consumed by the worker
	LagVelocity      float64 // dLag/dt (positive = growing, negative = shrinking)
	ProjectedBreachS float64 // seconds until lag reaches SLAThreshold (math.Inf if never)
	SLAThresholdEvents uint64
}

func (r PartitionLagReport) String() string {
	return fmt.Sprintf("Partition[%d] lag=%d eps_prod=%.0f eps_cons=%.0f velocity=%.1f/s breach=%.0fs",
		r.PartitionID, r.LagEvents, r.ProductionEPS, r.ConsumptionEPS,
		r.LagVelocity, r.ProjectedBreachS)
}

// partitionState holds running statistics for one partition.
type partitionState struct {
	logEndOffset   uint64
	currentOffset  uint64
	lastSampleTime time.Time
	lastEndOffset  uint64
	lastCurOffset  uint64
	productionEPS  float64
	consumptionEPS float64
	slaThreshold   uint64
}

// ConsumerLagMonitor tracks partition offsets and estimates lag velocity.
type ConsumerLagMonitor struct {
	mu         sync.Mutex
	partitions map[int]*partitionState
}

// NewConsumerLagMonitor constructs an empty monitor.
func NewConsumerLagMonitor() *ConsumerLagMonitor {
	return &ConsumerLagMonitor{
		partitions: make(map[int]*partitionState),
	}
}

// RegisterPartition adds a partition to the monitor with an SLA threshold.
func (m *ConsumerLagMonitor) RegisterPartition(partitionID int, slaThresholdEvents uint64) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.partitions[partitionID] = &partitionState{
		lastSampleTime: time.Now(),
		slaThreshold:   slaThresholdEvents,
	}
}

// UpdateOffsets records new offset observations for partitionID.
// logEndOffset is the broker's end offset; currentOffset is the consumer's position.
func (m *ConsumerLagMonitor) UpdateOffsets(partitionID int, logEndOffset, currentOffset uint64) {
	m.mu.Lock()
	defer m.mu.Unlock()

	ps, ok := m.partitions[partitionID]
	if !ok {
		return
	}

	now := time.Now()
	elapsed := now.Sub(ps.lastSampleTime).Seconds()
	if elapsed <= 0 {
		elapsed = 0.001 // guard against zero-division
	}

	// Compute per-second rates from delta since last sample
	if ps.lastSampleTime.IsZero() {
		ps.productionEPS = 0
		ps.consumptionEPS = 0
	} else {
		producedDelta := float64(logEndOffset) - float64(ps.lastEndOffset)
		consumedDelta := float64(currentOffset) - float64(ps.lastCurOffset)
		if producedDelta < 0 {
			producedDelta = 0
		}
		if consumedDelta < 0 {
			consumedDelta = 0
		}
		ps.productionEPS = producedDelta / elapsed
		ps.consumptionEPS = consumedDelta / elapsed
	}

	ps.lastEndOffset = logEndOffset
	ps.lastCurOffset = currentOffset
	ps.logEndOffset = logEndOffset
	ps.currentOffset = currentOffset
	ps.lastSampleTime = now
}

// CalculateLag returns a PartitionLagReport for partitionID.
func (m *ConsumerLagMonitor) CalculateLag(partitionID int) (PartitionLagReport, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()

	ps, ok := m.partitions[partitionID]
	if !ok {
		return PartitionLagReport{}, false
	}

	var lag uint64
	if ps.logEndOffset > ps.currentOffset {
		lag = ps.logEndOffset - ps.currentOffset
	}

	velocity := ps.productionEPS - ps.consumptionEPS // positive = lag growing

	var breachSec float64
	if velocity > 0 && ps.slaThreshold > lag {
		// Time to breach = remaining headroom / velocity
		breachSec = float64(ps.slaThreshold-lag) / velocity
	} else if velocity <= 0 {
		breachSec = 1e18 // effectively never
	} else {
		breachSec = 0 // already breached
	}

	return PartitionLagReport{
		PartitionID:        partitionID,
		LogEndOffset:       ps.logEndOffset,
		CurrentOffset:      ps.currentOffset,
		LagEvents:          lag,
		ProductionEPS:      ps.productionEPS,
		ConsumptionEPS:     ps.consumptionEPS,
		LagVelocity:        velocity,
		ProjectedBreachS:   breachSec,
		SLAThresholdEvents: ps.slaThreshold,
	}, true
}
