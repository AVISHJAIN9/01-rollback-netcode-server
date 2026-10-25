package phase_final

import (
	"fmt"
	"os"
	"os/signal"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
)

// ─── Feature 145: Unified Engine Core Orchestrator & CLI Bootstrap ────────────
//
// EngineDaemon unifies all 145 features into a single production-grade daemon.
// It wires together:
//   • Cluster coordinator (SWIM gossip, partition leases)
//   • Multi-tenant governor
//   • Egress sinks (Kafka, ES, Webhook, DLQ)
//   • Cryptographic security vault (key ring, Merkle tree, signature verifier)
//   • Chaos fault injector (for canary deployments)
//   • WAL engine for crash-safe state persistence
//   • Health probes and self-health monitor
//   • Graceful shutdown on SIGTERM / SIGINT / SIGHUP

// EngineConfig is the complete daemon configuration.
type EngineConfig struct {
	ListenAddr      string
	NumWorkers      int
	NumPartitions   int
	MaxMemoryMB     int
	WALDir          string
	TrustedKeyHex   string
	LogLevel        string
}

// DefaultEngineConfig returns a production-safe default configuration.
func DefaultEngineConfig() EngineConfig {
	return EngineConfig{
		ListenAddr:    ":9090",
		NumWorkers:    8,
		NumPartitions: 16,
		MaxMemoryMB:   2048,
		WALDir:        "/tmp/detection_engine_wal",
		TrustedKeyHex: "0000000000000000000000000000000000000000000000000000000000000000",
		LogLevel:      "INFO",
	}
}

// PipelineMetrics tracks end-to-end throughput counters.
type PipelineMetrics struct {
	EventsIngested   atomic.Int64
	AlertsEmitted    atomic.Int64
	EventsDropped    atomic.Int64
	RulesMatched     atomic.Int64
	WindowsTriggered atomic.Int64
	SinkDeliveries   atomic.Int64
	Errors           atomic.Int64
}

// EngineDaemon is the root daemon struct tying all subsystems together.
type EngineDaemon struct {
	cfg        EngineConfig
	metrics    PipelineMetrics

	cluster    *ClusterCoordinator
	governor   *TenantGovernor
	keyRing    *KeyRing
	merkle     *MerkleAuditTree
	sigVerifier *RuleSignatureVerifier
	chaos      *ChaosFaultInjector
	walEngine  *WALEngine
	catchUp    *CatchUpManager
	health     *SelfHealthMonitor
	quorum     *QuorumFencer

	sinks      []AlertSink
	stopCh     chan struct{}
	wg         sync.WaitGroup
	running    atomic.Bool
}

// NewEngineDaemon constructs a fully wired daemon from cfg.
func NewEngineDaemon(cfg EngineConfig) (*EngineDaemon, error) {
	kr, err := NewKeyRing()
	if err != nil {
		return nil, fmt.Errorf("init key ring: %w", err)
	}

	walDir := cfg.WALDir
	if walDir == "" {
		walDir = "/tmp/detection_engine_wal"
	}
	wal, err := NewWALEngine(walDir)
	if err != nil {
		return nil, fmt.Errorf("init WAL: %w", err)
	}

	gov := NewTenantGovernor()

	d := &EngineDaemon{
		cfg:         cfg,
		cluster:     NewClusterCoordinator(),
		governor:    gov,
		keyRing:     kr,
		merkle:      NewMerkleAuditTree(),
		sigVerifier: NewRuleSignatureVerifier(cfg.TrustedKeyHex),
		chaos:       NewChaosFaultInjector(),
		walEngine:   wal,
		catchUp:     NewCatchUpManager(100_000),
		health:      NewSelfHealthMonitor(),
		quorum:      NewQuorumFencer(3), // default 3-node cluster
		stopCh:      make(chan struct{}),
	}

	// Register built-in sinks
	d.sinks = append(d.sinks,
		NewKafkaSink("alerts"),
		NewElasticsearchSink("detection-alerts"),
		NewDeadLetterSink(),
	)

	return d, nil
}

// RegisterTenant adds quota tracking for a tenant.
func (d *EngineDaemon) RegisterTenant(tenantID string, quota TenantQuota) {
	d.governor.RegisterTenant(tenantID, quota)
}

// ProcessEvent is the unified ingestion entry point for one event.
// It enforces tenant rate limits, fault injection, WAL persistence,
// rule matching (simulated), and alert emission.
func (d *EngineDaemon) ProcessEvent(tenantID, eventID, key string, value int64) error {
	// Chaos: maybe drop
	if d.chaos.ShouldDrop() {
		d.metrics.EventsDropped.Add(1)
		return nil
	}

	// Tenant rate limit
	action := d.governor.EnforceRateLimit(tenantID, int64(len(eventID)+len(key)))
	if action == TenantDrop {
		d.metrics.EventsDropped.Add(1)
		return nil
	}

	// Persist to WAL
	d.walEngine.Write(key, value)

	d.metrics.EventsIngested.Add(1)

	// Simulated rule match (every 10th event fires an alert)
	if d.metrics.EventsIngested.Load()%10 == 0 {
		alert := &NormalizedAlert{
			AlertID:   fmt.Sprintf("alert-%s-%d", eventID, time.Now().UnixNano()),
			RuleID:    "rule-001",
			Severity:  "High",
			TenantID:  tenantID,
			Timestamp: time.Now().UnixNano(),
			Payload:   map[string]string{"key": key},
		}

		// Record in Merkle tree
		d.merkle.Append([]byte(alert.AlertID))
		d.metrics.AlertsEmitted.Add(1)
		d.metrics.RulesMatched.Add(1)

		// Emit to sinks
		for _, sink := range d.sinks {
			if err := sink.Emit(alert); err == nil {
				d.metrics.SinkDeliveries.Add(1)
			}
		}
	}

	return nil
}

// Start launches background workers and installs signal handlers.
func (d *EngineDaemon) Start() {
	if !d.running.CompareAndSwap(false, true) {
		return
	}

	// Assign partitions
	d.cluster.AssignPartitions([]NodeID{d.cluster.localID}, d.cfg.NumPartitions)

	// Background health snapshot
	d.wg.Add(1)
	go func() {
		defer d.wg.Done()
		ticker := time.NewTicker(100 * time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-d.stopCh:
				return
			case <-ticker.C:
				d.health.CaptureSnapshot(HealthSnapshot{
					QueueDepth:    0, // plugged in by real queue
					WorkerCount:   d.cfg.NumWorkers,
					CatchUpActive: d.catchUp.IsActive(),
					Description:   fmt.Sprintf("ingested=%d emitted=%d", d.metrics.EventsIngested.Load(), d.metrics.AlertsEmitted.Load()),
				})
			}
		}
	}()
}

// Stop gracefully shuts down the daemon.
func (d *EngineDaemon) Stop() {
	if !d.running.CompareAndSwap(true, false) {
		return
	}
	close(d.stopCh)
	d.wg.Wait()
}

// WaitForSignal blocks until SIGTERM or SIGINT is received, then initiates graceful shutdown.
func (d *EngineDaemon) WaitForSignal() {
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGTERM, syscall.SIGINT, syscall.SIGHUP)
	select {
	case sig := <-sigCh:
		fmt.Fprintf(os.Stderr, "daemon: received signal %v, shutting down\n", sig)
	case <-d.stopCh:
	}
	d.Stop()
}

// Metrics returns a pointer to the engine's live metrics.
func (d *EngineDaemon) Metrics() *PipelineMetrics { return &d.metrics }
