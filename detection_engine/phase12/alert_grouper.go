package phase12

import (
	"fmt"
	"sync"
)

// ─── Feature 71: Multi-Dimensional Alert Grouping & Windowed Aggregation ─────
//
// AlertGroupingManager groups NormalizedAlerts by a composite key
// (TenantID + Device.Hostname + AttackTechnique).
//
// A group is "active" while alerts keep arriving within inactivityTimeoutNs of
// the last alert.  Once the inactivity window closes (detected by FlushExpired),
// the completed group is emitted to the caller via the emit function.
//
// aggregated_risk = max of individual alert risk scores within the group.

// AlertGroup is a coalesced incident aggregating multiple related alerts.
type AlertGroup struct {
	GroupID        string
	Key            string
	Alerts         []NormalizedAlert
	FirstSeenNs    int64
	LastSeenNs     int64
	AggregatedRisk uint8
	Count          int
}

// AlertGroupingManager maintains active alert groups.
type AlertGroupingManager struct {
	mu                    sync.Mutex
	groups                map[string]*AlertGroup
	groupWindowDurationNs int64
	inactivityTimeoutNs   int64
}

// NewAlertGroupingManager constructs a manager with the given timing parameters.
func NewAlertGroupingManager(groupWindowDurationNs, inactivityTimeoutNs int64) *AlertGroupingManager {
	return &AlertGroupingManager{
		groups:                make(map[string]*AlertGroup),
		groupWindowDurationNs: groupWindowDurationNs,
		inactivityTimeoutNs:   inactivityTimeoutNs,
	}
}

// groupKey derives a deterministic composite key from an alert.
func groupKey(alert *NormalizedAlert) string {
	return fmt.Sprintf("%s|%s|%s", alert.TenantID, alert.Device.Hostname, alert.AttackTechnique)
}

// IngestAlert adds alert to the appropriate group, creating one if necessary.
// emitFn is NOT called during this method; call FlushExpired to emit closed groups.
func (m *AlertGroupingManager) IngestAlert(alert NormalizedAlert) {
	m.mu.Lock()
	defer m.mu.Unlock()

	key := groupKey(&alert)

	if g, ok := m.groups[key]; ok {
		g.Alerts = append(g.Alerts, alert)
		g.LastSeenNs = alert.CreatedTimeNs
		g.Count++
		if alert.RiskScore > g.AggregatedRisk {
			g.AggregatedRisk = alert.RiskScore
		}
	} else {
		m.groups[key] = &AlertGroup{
			GroupID:        newUUID(),
			Key:            key,
			Alerts:         []NormalizedAlert{alert},
			FirstSeenNs:    alert.CreatedTimeNs,
			LastSeenNs:     alert.CreatedTimeNs,
			AggregatedRisk: alert.RiskScore,
			Count:          1,
		}
	}
}

// FlushExpired emits and removes groups whose last activity was more than
// inactivityTimeoutNs nanoseconds ago (relative to nowNs).
// Returns all flushed groups.
func (m *AlertGroupingManager) FlushExpired(nowNs int64) []*AlertGroup {
	m.mu.Lock()
	defer m.mu.Unlock()

	var flushed []*AlertGroup
	for key, group := range m.groups {
		if nowNs-group.LastSeenNs >= m.inactivityTimeoutNs {
			flushed = append(flushed, group)
			delete(m.groups, key)
		}
	}
	return flushed
}

// ActiveGroupCount returns the number of currently active groups.
func (m *AlertGroupingManager) ActiveGroupCount() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.groups)
}

// GetGroup returns a copy of the active group for key (nil if not found).
func (m *AlertGroupingManager) GetGroup(tenantID, hostname, technique string) *AlertGroup {
	m.mu.Lock()
	defer m.mu.Unlock()
	key := fmt.Sprintf("%s|%s|%s", tenantID, hostname, technique)
	if g, ok := m.groups[key]; ok {
		cp := *g
		return &cp
	}
	return nil
}
