package phase_final

import (
	"fmt"
	"sync"
	"sync/atomic"
	"time"
)

// ─── Features 127–132: Multi-Tenancy, Resource Isolation & Governance ─────────

// TenantAction is the result of a rate-limit evaluation.
type TenantAction int

const (
	TenantAllow    TenantAction = iota
	TenantThrottle              // allow with mandatory delay
	TenantDrop                  // reject — quota exceeded
)

func (t TenantAction) String() string {
	switch t {
	case TenantAllow:
		return "Allow"
	case TenantThrottle:
		return "Throttle"
	case TenantDrop:
		return "Drop"
	default:
		return "Unknown"
	}
}

// TenantQuota describes the resource limits for one tenant.
type TenantQuota struct {
	MaxMemoryBytes  int64
	SustainedEPS    float64 // sustained events per second
	BurstEPS        float64 // burst cap
}

// tenantBucket is the per-tenant runtime state.
type tenantBucket struct {
	quota          TenantQuota
	memUsed        atomic.Int64
	tokens         atomic.Int64  // current token balance (scaled by 1000)
	lastRefillNs   atomic.Int64
	totalEvents    atomic.Int64
	droppedEvents  atomic.Int64
}

func newTenantBucket(q TenantQuota) *tenantBucket {
	b := &tenantBucket{quota: q}
	b.tokens.Store(int64(q.BurstEPS * 1000))
	b.lastRefillNs.Store(time.Now().UnixNano())
	return b
}

func (b *tenantBucket) refill(nowNs int64) {
	last := b.lastRefillNs.Load()
	elapsed := float64(nowNs-last) / 1e9
	if elapsed <= 0 {
		return
	}
	earned := int64(elapsed * b.quota.SustainedEPS * 1000)
	if earned == 0 {
		return
	}
	if b.lastRefillNs.CompareAndSwap(last, nowNs) {
		for {
			cur := b.tokens.Load()
			max := int64(b.quota.BurstEPS * 1000)
			next := cur + earned
			if next > max {
				next = max
			}
			if b.tokens.CompareAndSwap(cur, next) {
				return
			}
		}
	}
}

// TenantGovernor enforces per-tenant resource quotas.
type TenantGovernor struct {
	mu      sync.RWMutex
	buckets map[string]*tenantBucket
}

// NewTenantGovernor constructs an empty governor.
func NewTenantGovernor() *TenantGovernor {
	return &TenantGovernor{buckets: make(map[string]*tenantBucket)}
}

// RegisterTenant sets up quota tracking for a tenant.
func (g *TenantGovernor) RegisterTenant(tenantID string, quota TenantQuota) {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.buckets[tenantID] = newTenantBucket(quota)
}

// EnforceRateLimit evaluates whether a tenant may ingest eventBytes of data.
// Returns TenantAllow, TenantThrottle, or TenantDrop.
func (g *TenantGovernor) EnforceRateLimit(tenantID string, eventBytes int64) TenantAction {
	g.mu.RLock()
	b, ok := g.buckets[tenantID]
	g.mu.RUnlock()
	if !ok {
		return TenantDrop // unknown tenant
	}

	nowNs := time.Now().UnixNano()
	b.refill(nowNs)
	b.totalEvents.Add(1)

	// Memory quota check
	if b.memUsed.Load()+eventBytes > b.quota.MaxMemoryBytes {
		b.droppedEvents.Add(1)
		return TenantDrop
	}

	// Token bucket check
	for {
		cur := b.tokens.Load()
		if cur < 1000 { // less than 1 token
			b.droppedEvents.Add(1)
			return TenantThrottle
		}
		if b.tokens.CompareAndSwap(cur, cur-1000) {
			b.memUsed.Add(eventBytes)
			return TenantAllow
		}
	}
}

// RecordMemoryFree reduces the tracked memory for tenantID.
func (g *TenantGovernor) RecordMemoryFree(tenantID string, bytes int64) {
	g.mu.RLock()
	b, ok := g.buckets[tenantID]
	g.mu.RUnlock()
	if ok {
		b.memUsed.Add(-bytes)
	}
}

// Stats returns (totalEvents, droppedEvents) for a tenant.
func (g *TenantGovernor) Stats(tenantID string) (int64, int64) {
	g.mu.RLock()
	b, ok := g.buckets[tenantID]
	g.mu.RUnlock()
	if !ok {
		return 0, 0
	}
	return b.totalEvents.Load(), b.droppedEvents.Load()
}

// ─── Feature 130: RBAC Token Validator ───────────────────────────────────────

// Role defines an operational permission scope.
type Role int

const (
	RoleRuleAuthor     Role = iota // compile/test rules
	RoleSecurityAnalyst            // view alerts, ack, annotate
	RoleEngineOperator             // flush queues, drain nodes
	RoleAuditor                    // read-only audit trails
)

func (r Role) String() string {
	switch r {
	case RoleRuleAuthor:
		return "RuleAuthor"
	case RoleSecurityAnalyst:
		return "SecurityAnalyst"
	case RoleEngineOperator:
		return "EngineOperator"
	case RoleAuditor:
		return "Auditor"
	default:
		return fmt.Sprintf("Role(%d)", int(r))
	}
}

// RBACToken is a simplified bearer token carrying identity and role claims.
type RBACToken struct {
	Subject  string
	TenantID string
	Role     Role
	// In production: Ed25519 signature over (Subject + TenantID + Role + ExpiryNs)
	ExpiryNs int64
	Valid     bool // set by Validate
}

// RBACValidator verifies token authenticity (simplified: checks expiry and role).
type RBACValidator struct{}

// Validate checks token validity and sets token.Valid.
func (v *RBACValidator) Validate(token *RBACToken) bool {
	if token.ExpiryNs > 0 && time.Now().UnixNano() > token.ExpiryNs {
		token.Valid = false
		return false
	}
	token.Valid = true
	return true
}

// Authorize returns true when the token has the required role (or higher).
func (v *RBACValidator) Authorize(token *RBACToken, required Role) bool {
	return token.Valid && int(token.Role) <= int(required)
}
