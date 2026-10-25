package phase12

import "sync"

// ─── Feature 69: Contextual Alert Enrichment & Entity Resolution ─────────────
//
// AlertEnrichmentEngine maintains two in-memory thread-safe caches:
//   AssetDirectory – keyed by IP address or hostname → AssetRecord
//   UserDirectory  – keyed by user principal → UserRecord
//
// Enrich() resolves device and actor entities and injects metadata into
// alert.Enrichment.  All lookups are O(1) hash-map reads under a RWMutex.

// AssetRecord holds CMDB metadata for one asset.
type AssetRecord struct {
	Hostname         string
	IPAddress        string
	BusinessUnit     string
	Criticality      string // "Critical", "High", "Medium", "Low"
	OSVersion        string
	Owner            string
	NetworkZone      string
}

// UserRecord holds IAM metadata for one user principal.
type UserRecord struct {
	Principal        string
	DisplayName      string
	Department       string
	Manager          string
	PrivilegeTier    string // "Domain Admin", "Local Admin", "Standard"
	IsVIP            bool
	IsServiceAccount bool
	IsActive         bool
}

// AssetDirectory is a thread-safe in-memory CMDB cache.
type AssetDirectory struct {
	mu      sync.RWMutex
	byIP    map[string]*AssetRecord
	byHost  map[string]*AssetRecord
}

// NewAssetDirectory constructs an empty directory.
func NewAssetDirectory() *AssetDirectory {
	return &AssetDirectory{
		byIP:   make(map[string]*AssetRecord),
		byHost: make(map[string]*AssetRecord),
	}
}

// Register adds or updates a record (indexed by both IP and hostname).
func (ad *AssetDirectory) Register(r *AssetRecord) {
	ad.mu.Lock()
	defer ad.mu.Unlock()
	if r.IPAddress != "" {
		ad.byIP[r.IPAddress] = r
	}
	if r.Hostname != "" {
		ad.byHost[r.Hostname] = r
	}
}

// LookupByIP returns the asset record for the given IP, or nil.
func (ad *AssetDirectory) LookupByIP(ip string) *AssetRecord {
	ad.mu.RLock()
	defer ad.mu.RUnlock()
	return ad.byIP[ip]
}

// LookupByHostname returns the asset record for the given hostname, or nil.
func (ad *AssetDirectory) LookupByHostname(hostname string) *AssetRecord {
	ad.mu.RLock()
	defer ad.mu.RUnlock()
	return ad.byHost[hostname]
}

// UserDirectory is a thread-safe in-memory IAM cache.
type UserDirectory struct {
	mu    sync.RWMutex
	users map[string]*UserRecord
}

// NewUserDirectory constructs an empty directory.
func NewUserDirectory() *UserDirectory {
	return &UserDirectory{users: make(map[string]*UserRecord)}
}

// Register adds or updates a user record by principal name.
func (ud *UserDirectory) Register(r *UserRecord) {
	ud.mu.Lock()
	defer ud.mu.Unlock()
	ud.users[r.Principal] = r
}

// Lookup returns the user record for the given principal, or nil.
func (ud *UserDirectory) Lookup(principal string) *UserRecord {
	ud.mu.RLock()
	defer ud.mu.RUnlock()
	return ud.users[principal]
}

// AlertEnrichmentEngine performs contextual enrichment of NormalizedAlert objects.
type AlertEnrichmentEngine struct {
	Assets *AssetDirectory
	Users  *UserDirectory
}

// NewAlertEnrichmentEngine constructs an engine with empty directories.
func NewAlertEnrichmentEngine() *AlertEnrichmentEngine {
	return &AlertEnrichmentEngine{
		Assets: NewAssetDirectory(),
		Users:  NewUserDirectory(),
	}
}

// Enrich resolves device and actor entities and decorates alert.Enrichment.
// All lookups are in-memory: latency is bounded by map read time (< 1µs).
func (e *AlertEnrichmentEngine) Enrich(alert *NormalizedAlert) {
	// Asset resolution: try hostname first, then IP
	var asset *AssetRecord
	if alert.Device.Hostname != "" {
		asset = e.Assets.LookupByHostname(alert.Device.Hostname)
	}
	if asset == nil && alert.Device.IPAddress != "" {
		asset = e.Assets.LookupByIP(alert.Device.IPAddress)
	}

	if asset != nil {
		alert.Enrichment.AssetOwner = asset.Owner
		alert.Enrichment.BusinessUnit = asset.BusinessUnit
		alert.Enrichment.AssetCriticality = asset.Criticality
		alert.Enrichment.NetworkZone = asset.NetworkZone
		alert.Enrichment.OSVersion = asset.OSVersion
		// Back-fill device fields that may have been missing
		if alert.Device.Hostname == "" {
			alert.Device.Hostname = asset.Hostname
		}
	}

	// User resolution
	if alert.Actor.UserPrincipal != "" {
		if user := e.Users.Lookup(alert.Actor.UserPrincipal); user != nil {
			alert.Enrichment.Department = user.Department
			alert.Enrichment.Manager = user.Manager
			alert.Enrichment.PrivilegeTier = user.PrivilegeTier
			alert.Enrichment.IsVIP = user.IsVIP
			alert.Enrichment.IsServiceAccount = user.IsServiceAccount
			alert.Actor.UserDisplayName = user.DisplayName
		}
	}
}
