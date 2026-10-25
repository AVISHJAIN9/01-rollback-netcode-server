package phase12

import (
	"fmt"
	"regexp"
	"testing"
	"time"
)

// ─── Test 1: Alert Schema Normalization ───────────────────────────────────────

func TestAlertSchemaNormalization(t *testing.T) {
	t.Log("Test 1: Alert Schema Normalization")

	normalizer := NewAlertNormalizer()

	match := &DetectionMatch{
		MatchID:         "raw-match-001",
		RuleID:          "sigma-ps-exec-001",
		RuleTitle:       "Suspicious PowerShell Execution",
		Severity:        SeverityHigh,
		TimestampNs:     time.Now().UnixNano(),
		EntityID:        "host-dc01",
		SourceIP:        "10.0.10.5",
		DestIP:          "192.168.1.200",
		DestPort:        445,
		Username:        "jsmith",
		Hostname:        "prod-db-01",
		ProcessName:     "powershell.exe",
		ProcessID:       4321,
		RawEventID:      "evt-raw-001",
		RawPayload:      map[string]string{"CommandLine": "powershell -enc BASE64"},
		TenantID:        "tenant-acme",
		AttackTechnique: "T1059.001",
		Tags:            []string{"execution", "powershell"},
	}

	alert := normalizer.Normalize(match)

	// Validate non-empty required fields
	if alert.AlertID == "" {
		t.Fatal("FAIL: AlertID is empty")
	}
	if alert.RuleID != match.RuleID {
		t.Fatalf("FAIL: RuleID=%s, expected %s", alert.RuleID, match.RuleID)
	}
	if alert.Title != match.RuleTitle {
		t.Fatalf("FAIL: Title=%s, expected %s", alert.Title, match.RuleTitle)
	}
	if alert.Severity != SeverityHigh {
		t.Fatalf("FAIL: Severity=%s, expected High", alert.Severity)
	}
	if alert.RiskScore == 0 {
		t.Fatal("FAIL: RiskScore is 0")
	}

	// Validate actor block
	if alert.Actor.UserPrincipal != "jsmith" {
		t.Fatalf("FAIL: Actor.UserPrincipal=%s", alert.Actor.UserPrincipal)
	}
	if alert.Actor.ProcessName != "powershell.exe" {
		t.Fatalf("FAIL: Actor.ProcessName=%s", alert.Actor.ProcessName)
	}

	// Validate device block
	if alert.Device.Hostname != "prod-db-01" {
		t.Fatalf("FAIL: Device.Hostname=%s", alert.Device.Hostname)
	}
	if alert.Device.IPAddress != "10.0.10.5" {
		t.Fatalf("FAIL: Device.IPAddress=%s", alert.Device.IPAddress)
	}

	// Validate network block
	if alert.Network.DestPort != 445 {
		t.Fatalf("FAIL: Network.DestPort=%d", alert.Network.DestPort)
	}

	// Validate evidence block is non-empty
	if len(alert.Evidence) == 0 {
		t.Fatal("FAIL: Evidence block is empty")
	}
	if alert.Evidence[0].EventID != "evt-raw-001" {
		t.Fatalf("FAIL: Evidence[0].EventID=%s", alert.Evidence[0].EventID)
	}
	if len(alert.Evidence[0].RawPayload) == 0 {
		t.Fatal("FAIL: Evidence[0].RawPayload is empty")
	}

	// Validate status and metadata
	if alert.Status != AlertStatusActive {
		t.Fatalf("FAIL: Status=%s, expected Active", alert.Status)
	}
	if alert.TenantID != "tenant-acme" {
		t.Fatalf("FAIL: TenantID=%s", alert.TenantID)
	}

	t.Logf("  ✓ AlertID=%s", alert.AlertID)
	t.Logf("  ✓ Title=%s, Severity=%s, RiskScore=%d", alert.Title, alert.Severity, alert.RiskScore)
	t.Logf("  ✓ Actor=%s / %s, Device=%s, Evidence=%d records",
		alert.Actor.UserPrincipal, alert.Actor.ProcessName, alert.Device.Hostname, len(alert.Evidence))
}

// ─── Test 2: Dynamic Severity Elevation ──────────────────────────────────────

func TestDynamicSeverityElevation(t *testing.T) {
	t.Log("Test 2: Dynamic Severity Elevation")

	scorer := NewSeverityScorer()

	// Base: Medium (50), Asset: DC (W_asset=1.8), User: Domain Admin (W_user=1.3), TI default (W_ti=1.0)
	score, severity := scorer.ComputeRiskScore(SeverityMedium, 1.8, 1.3, 1.0)

	// Expected: 50 × 1.8 × 1.3 × 1.0 = 117 → clamped to 100 → Critical
	if score < 85 {
		t.Fatalf("FAIL: risk score %d < 85 (expected Critical escalation)", score)
	}
	if severity != SeverityCritical {
		t.Fatalf("FAIL: severity=%s, expected Critical", severity)
	}

	// Boundary: Medium × low weights should stay Medium
	score2, sev2 := scorer.ComputeRiskScore(SeverityMedium, 1.0, 1.0, 1.0)
	if score2 != 50 {
		t.Fatalf("FAIL: unweighted score=%d, expected 50", score2)
	}
	if sev2 != SeverityMedium {
		t.Fatalf("FAIL: unweighted severity=%s, expected Medium", sev2)
	}

	// Low severity elevated to High by critical asset (Low=25, W_asset=3.0, W_user=1.0, W_ti=1.0 = 75)
	score3, sev3 := scorer.ComputeRiskScore(SeverityLow, 3.0, 1.0, 1.0)
	if sev3 != SeverityHigh {
		t.Fatalf("FAIL: Low×3.0 severity=%s (score=%d), expected High", sev3, score3)
	}

	t.Logf("  ✓ Medium(50) × W_asset=1.8 × W_user=1.3 → score=%d, severity=%s", score, severity)
	t.Logf("  ✓ Unweighted score=50 → Medium (no escalation)")
	t.Logf("  ✓ Low(25) × W_asset=3.0 → score=%d, severity=%s (escalated)", score3, sev3)
}

// ─── Test 3: Contextual Asset & Identity Enrichment ──────────────────────────

func TestContextualEnrichment(t *testing.T) {
	t.Log("Test 3: Contextual Asset & Identity Enrichment")

	engine := NewAlertEnrichmentEngine()

	// Seed CMDB
	engine.Assets.Register(&AssetRecord{
		Hostname:     "prod-db-01",
		IPAddress:    "10.0.10.5",
		BusinessUnit: "Engineering",
		Criticality:  "High",
		OSVersion:    "Ubuntu 22.04",
		Owner:        "infra-team@acme.com",
		NetworkZone:  "Production",
	})

	// Seed IAM
	engine.Users.Register(&UserRecord{
		Principal:     "jsmith",
		DisplayName:   "John Smith",
		Department:    "Engineering",
		Manager:       "cto@acme.com",
		PrivilegeTier: "Standard",
		IsVIP:         true,
		IsActive:      true,
	})

	normalizer := NewAlertNormalizer()
	match := &DetectionMatch{
		RuleID:      "rule-001",
		RuleTitle:   "Test Alert",
		Severity:    SeverityMedium,
		TimestampNs: time.Now().UnixNano(),
		SourceIP:    "10.0.10.5",
		Hostname:    "prod-db-01",
		Username:    "jsmith",
		RawEventID:  "evt-enrich-001",
		RawPayload:  map[string]string{"action": "login"},
	}

	alert := normalizer.Normalize(match)
	engine.Enrich(alert)

	// Verify asset enrichment
	if alert.Enrichment.AssetCriticality != "High" {
		t.Fatalf("FAIL: AssetCriticality=%s, expected High", alert.Enrichment.AssetCriticality)
	}
	if alert.Enrichment.BusinessUnit != "Engineering" {
		t.Fatalf("FAIL: BusinessUnit=%s", alert.Enrichment.BusinessUnit)
	}
	if alert.Enrichment.NetworkZone != "Production" {
		t.Fatalf("FAIL: NetworkZone=%s", alert.Enrichment.NetworkZone)
	}
	if alert.Enrichment.AssetOwner != "infra-team@acme.com" {
		t.Fatalf("FAIL: AssetOwner=%s", alert.Enrichment.AssetOwner)
	}

	// Verify user enrichment
	if alert.Enrichment.Department != "Engineering" {
		t.Fatalf("FAIL: Department=%s", alert.Enrichment.Department)
	}
	if !alert.Enrichment.IsVIP {
		t.Fatal("FAIL: IsVIP should be true for jsmith")
	}
	if alert.Enrichment.PrivilegeTier != "Standard" {
		t.Fatalf("FAIL: PrivilegeTier=%s", alert.Enrichment.PrivilegeTier)
	}
	if alert.Actor.UserDisplayName != "John Smith" {
		t.Fatalf("FAIL: UserDisplayName=%s", alert.Actor.UserDisplayName)
	}

	t.Logf("  ✓ Asset: Hostname=prod-db-01, Criticality=%s, Zone=%s",
		alert.Enrichment.AssetCriticality, alert.Enrichment.NetworkZone)
	t.Logf("  ✓ User: Department=%s, IsVIP=%v, DisplayName=%s",
		alert.Enrichment.Department, alert.Enrichment.IsVIP, alert.Actor.UserDisplayName)
}

// ─── Test 4: Suppression Rule Filter Execution ────────────────────────────────

func TestSuppressionRuleFilter(t *testing.T) {
	t.Log("Test 4: Suppression Rule Filter Execution")

	gate := NewSuppressionGate()
	now := time.Now().UnixNano()

	// Rule: scanner IP during maintenance window
	scannerIP := "192.168.50.100"
	maintenanceStart := now - int64(time.Hour)
	maintenanceEnd := now + int64(time.Hour)

	gate.RegisterRule(&SuppressionRule{
		ID:   "sup-rule-scanner-001",
		Name: "Vulnerability Scanner During Maintenance",
		Predicates: []SuppressionPredicate{
			{Field: FieldSourceIP, Type: PredicateEquals, Literal: scannerIP},
		},
		ActiveTimeRanges: []TimeRange{
			{StartNs: maintenanceStart, EndNs: maintenanceEnd},
		},
		ExpirationNs: 0, // no expiry
	})

	// Rule: hostname matching test-* regex
	testHostPattern := regexp.MustCompile(`^test-.*`)
	gate.RegisterRule(&SuppressionRule{
		ID:   "sup-rule-test-hosts",
		Name: "Test Environment Hosts",
		Predicates: []SuppressionPredicate{
			{Field: FieldHostname, Type: PredicateRegex, Pattern: testHostPattern},
		},
	})

	normalizer := NewAlertNormalizer()

	// Alert from scanner IP — should be suppressed
	scannerMatch := &DetectionMatch{
		RuleID:      "rule-net-001",
		RuleTitle:   "Port Scan Detected",
		Severity:    SeverityMedium,
		TimestampNs: now,
		SourceIP:    scannerIP,
		Hostname:    "prod-server-01",
		RawEventID:  "evt-scan-001",
		RawPayload:  map[string]string{},
	}
	scannerAlert := normalizer.Normalize(scannerMatch)
	result := gate.EvaluateSuppression(scannerAlert, now)

	if !result.Suppressed {
		t.Fatal("FAIL: scanner IP alert was NOT suppressed")
	}
	if result.RuleID != "sup-rule-scanner-001" {
		t.Fatalf("FAIL: suppressed by wrong rule: %s", result.RuleID)
	}
	if scannerAlert.Status != AlertStatusSuppressed {
		t.Fatalf("FAIL: alert.Status=%s, expected Suppressed", scannerAlert.Status)
	}
	if scannerAlert.Enrichment.SuppressedByRule != "sup-rule-scanner-001" {
		t.Fatalf("FAIL: SuppressedByRule=%s", scannerAlert.Enrichment.SuppressedByRule)
	}

	// Alert from test host — should be suppressed by regex rule
	testMatch := &DetectionMatch{
		RuleID:     "rule-net-001",
		RuleTitle:  "Port Scan",
		Severity:   SeverityLow,
		TimestampNs: now,
		SourceIP:   "10.1.1.1",
		Hostname:   "test-sandbox-01",
		RawEventID: "evt-test-001",
		RawPayload: map[string]string{},
	}
	testAlert := normalizer.Normalize(testMatch)
	result2 := gate.EvaluateSuppression(testAlert, now)
	if !result2.Suppressed {
		t.Fatal("FAIL: test-host alert was NOT suppressed by regex rule")
	}
	if result2.RuleID != "sup-rule-test-hosts" {
		t.Fatalf("FAIL: suppressed by wrong rule: %s", result2.RuleID)
	}

	// Normal production alert — should NOT be suppressed
	prodMatch := &DetectionMatch{
		RuleID:     "rule-net-001",
		RuleTitle:  "Lateral Movement",
		Severity:   SeverityHigh,
		TimestampNs: now,
		SourceIP:   "10.0.5.20",
		Hostname:   "prod-server-03",
		RawEventID: "evt-prod-001",
		RawPayload: map[string]string{},
	}
	prodAlert := normalizer.Normalize(prodMatch)
	result3 := gate.EvaluateSuppression(prodAlert, now)
	if result3.Suppressed {
		t.Fatal("FAIL: production alert was incorrectly suppressed")
	}
	if prodAlert.Status != AlertStatusActive {
		t.Fatalf("FAIL: prod alert status=%s, expected Active", prodAlert.Status)
	}

	t.Logf("  ✓ Scanner IP alert suppressed by rule=%s", result.RuleID)
	t.Logf("  ✓ Test-host alert suppressed by rule=%s (regex)", result2.RuleID)
	t.Logf("  ✓ Production alert NOT suppressed (status=Active)")
}

// ─── Test 5: Multi-Dimensional Alert Grouping ─────────────────────────────────

func TestMultiDimensionalAlertGrouping(t *testing.T) {
	t.Log("Test 5: Multi-Dimensional Alert Grouping")

	const windowNs = int64(5 * time.Minute)
	const inactivityNs = int64(30 * time.Second)
	manager := NewAlertGroupingManager(windowNs, inactivityNs)

	normalizer := NewAlertNormalizer()
	baseTime := time.Now().UnixNano()

	// Ingest 30 alerts for host-42 over 2 minutes (4-second spacing)
	for i := 0; i < 30; i++ {
		ts := baseTime + int64(i)*4*int64(time.Second)
		match := &DetectionMatch{
			RuleID:          "rule-lateral-001",
			RuleTitle:       "Lateral Movement Detected",
			Severity:        SeverityHigh,
			TimestampNs:     ts,
			Hostname:        "host-42",
			SourceIP:        "10.0.1.42",
			RawEventID:      fmt.Sprintf("evt-%03d", i),
			RawPayload:      map[string]string{},
			TenantID:        "tenant-acme",
			AttackTechnique: "T1021.002",
		}
		alert := normalizer.Normalize(match)
		alert.CreatedTimeNs = ts
		alert.RiskScore = 75
		manager.IngestAlert(*alert)
	}

	// Verify exactly one active group
	if manager.ActiveGroupCount() != 1 {
		t.Fatalf("FAIL: expected 1 active group, got %d", manager.ActiveGroupCount())
	}

	g := manager.GetGroup("tenant-acme", "host-42", "T1021.002")
	if g == nil {
		t.Fatal("FAIL: could not retrieve group for host-42 / T1021.002")
	}
	if g.Count != 30 {
		t.Fatalf("FAIL: group count=%d, expected 30", g.Count)
	}
	if len(g.Alerts) != 30 {
		t.Fatalf("FAIL: group.Alerts len=%d, expected 30", len(g.Alerts))
	}
	if g.AggregatedRisk != 75 {
		t.Fatalf("FAIL: AggregatedRisk=%d, expected 75", g.AggregatedRisk)
	}

	// Verify time span covers 2 minutes: last event at baseTime + 29*4s = +116s
	expectedDuration := int64(29) * 4 * int64(time.Second)
	actualDuration := g.LastSeenNs - g.FirstSeenNs
	if actualDuration < expectedDuration-int64(time.Second) || actualDuration > expectedDuration+int64(time.Second) {
		t.Fatalf("FAIL: group duration=%dns, expected ~%dns", actualDuration, expectedDuration)
	}

	// Flush after inactivity timeout
	flushTime := g.LastSeenNs + inactivityNs + int64(time.Second)
	flushed := manager.FlushExpired(flushTime)
	if len(flushed) != 1 {
		t.Fatalf("FAIL: expected 1 flushed group, got %d", len(flushed))
	}
	if manager.ActiveGroupCount() != 0 {
		t.Fatalf("FAIL: expected 0 active groups after flush, got %d", manager.ActiveGroupCount())
	}

	t.Logf("  ✓ 30 alerts coalesced into 1 group (count=%d, risk=%d)",
		flushed[0].Count, flushed[0].AggregatedRisk)
	t.Logf("  ✓ Group duration=%.1fs, FirstSeen→LastSeen span correct",
		float64(actualDuration)/float64(time.Second))
	t.Logf("  ✓ Group flushed after inactivity timeout")
}

// ─── Test 6: Cross-Alert Temporal Correlation Linking ─────────────────────────

func TestCrossAlertCorrelationLinking(t *testing.T) {
	t.Log("Test 6: Cross-Alert Temporal Correlation Linking")

	// 10-minute epoch width for correlation
	epochNs := int64(10 * time.Minute)
	engine := NewCorrelationEngine(epochNs)

	normalizer := NewAlertNormalizer()

	// Anchor baseTime to the START of the current epoch so that +5min always
	// lands in the same epoch bucket, and +13min always lands in the next one.
	now := time.Now().UnixNano()
	epochStart := (now / epochNs) * epochNs          // start of current epoch
	baseTime := time.Unix(0, epochStart+int64(time.Minute)) // 1 minute into epoch

	// Alert 1: suspicious process execution on host-dc01 at T=0
	match1 := &DetectionMatch{
		RuleID:          "rule-ps-exec-001",
		RuleTitle:       "Suspicious PowerShell Execution",
		Severity:        SeverityHigh,
		TimestampNs:     baseTime.UnixNano(),
		Hostname:        "host-dc01",
		SourceIP:        "10.0.10.5",
		RawEventID:      "evt-001",
		RawPayload:      map[string]string{"process": "powershell.exe"},
		TenantID:        "tenant-acme",
		AttackTechnique: "T1059.001",
	}
	alert1 := normalizer.Normalize(match1)
	alert1.CreatedTimeNs = baseTime.UnixNano()

	// Alert 2: lateral SMB connection from same host, 5 minutes later (within epoch)
	match2 := &DetectionMatch{
		RuleID:          "rule-smb-lateral-001",
		RuleTitle:       "Lateral SMB Connection",
		Severity:        SeverityCritical,
		TimestampNs:     baseTime.Add(5 * time.Minute).UnixNano(),
		Hostname:        "host-dc01", // same entity
		SourceIP:        "10.0.10.5",
		RawEventID:      "evt-002",
		RawPayload:      map[string]string{"dest_port": "445"},
		TenantID:        "tenant-acme",
		AttackTechnique: "T1021.002",
	}
	alert2 := normalizer.Normalize(match2)
	alert2.CreatedTimeNs = baseTime.Add(5 * time.Minute).UnixNano()

	// Alert 3: different host, same epoch — must get a DIFFERENT correlation ID
	match3 := &DetectionMatch{
		RuleID:          "rule-ps-exec-001",
		RuleTitle:       "Suspicious PowerShell Execution",
		Severity:        SeverityHigh,
		TimestampNs:     baseTime.UnixNano(),
		Hostname:        "host-workstation-99", // different entity
		SourceIP:        "10.0.10.99",
		RawEventID:      "evt-003",
		RawPayload:      map[string]string{"process": "powershell.exe"},
		TenantID:        "tenant-acme",
		AttackTechnique: "T1059.001",
	}
	alert3 := normalizer.Normalize(match3)
	alert3.CreatedTimeNs = baseTime.UnixNano()

	// Assign correlation IDs
	corr1 := engine.AssignCorrelationID(alert1)
	corr2 := engine.AssignCorrelationID(alert2)
	corr3 := engine.AssignCorrelationID(alert3)

	// Alert1 and Alert2 share host-dc01 within the same 10-minute epoch → same ID
	if corr1 != corr2 {
		t.Fatalf("FAIL: alert1 correlation=%016x, alert2 correlation=%016x — must be equal (same host, same epoch)",
			corr1, corr2)
	}

	// Alert3 is on a different host → different ID
	if corr3 == corr1 {
		t.Fatalf("FAIL: alert3 got same correlation as alert1/2 (%016x), expected different", corr3)
	}

	// Determinism: calling again with the same inputs must return the same token
	corr1b := engine.AssignCorrelationID(alert1)
	if corr1b != corr1 {
		t.Fatalf("FAIL: repeated assignment non-deterministic: %016x != %016x", corr1b, corr1)
	}

	// Alert4: same host but in NEXT epoch (13 minutes later) → different ID
	alert4 := normalizer.Normalize(match1)
	alert4.CreatedTimeNs = baseTime.Add(13 * time.Minute).UnixNano()
	corr4 := engine.AssignCorrelationID(alert4)
	if corr4 == corr1 {
		t.Fatalf("FAIL: alert in next epoch got same correlation ID — epochs must be isolated")
	}

	t.Logf("  ✓ alert1 (PS exec) correlationID = %016x", corr1)
	t.Logf("  ✓ alert2 (SMB lateral, +5min) correlationID = %016x (SAME — linked)", corr2)
	t.Logf("  ✓ alert3 (different host) correlationID = %016x (DIFFERENT)", corr3)
	t.Logf("  ✓ alert4 (next epoch) correlationID = %016x (DIFFERENT — epoch isolated)", corr4)
	t.Logf("  ✓ Determinism verified: repeated call returns same token")
	t.Logf("  ✓ Active token entries: %d", engine.ActiveTokenCount())
}
