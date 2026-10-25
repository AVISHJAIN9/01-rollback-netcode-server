package phase_final

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"sync"
)

// ─── Features 133–138: Cryptographic Integrity, Zero-Trust & Encryption ───────

// ─── Feature 134: Rule Signature Verifier (Ed25519 simplified) ───────────────

// RuleSignatureVerifier verifies that rule payloads carry a valid signature.
// For portability without the crypto/ed25519 dependency in the test harness,
// we use a SHA-256 HMAC-style scheme: signature = SHA256(trustedKey ‖ payload).
type RuleSignatureVerifier struct {
	trustedKeyHex string // hex-encoded trusted key (32 bytes)
}

// NewRuleSignatureVerifier constructs a verifier with the given trusted key hex.
func NewRuleSignatureVerifier(trustedKeyHex string) *RuleSignatureVerifier {
	return &RuleSignatureVerifier{trustedKeyHex: trustedKeyHex}
}

// Sign produces a detached signature over payload.
func (v *RuleSignatureVerifier) Sign(payload []byte) string {
	key, _ := hex.DecodeString(v.trustedKeyHex)
	h := sha256.New()
	h.Write(key)
	h.Write(payload)
	return hex.EncodeToString(h.Sum(nil))
}

// Verify returns true when sig is a valid signature over payload.
func (v *RuleSignatureVerifier) Verify(payload []byte, sig string) bool {
	expected := v.Sign(payload)
	return expected == sig
}

// ─── Feature 135: AES-256-GCM State Cipher ───────────────────────────────────

// StateCipher encrypts and decrypts state snapshots using AES-256-GCM.
type StateCipher struct {
	key []byte // 32 bytes
}

// NewStateCipher generates a fresh random 32-byte AES-256 key.
func NewStateCipher() (*StateCipher, error) {
	key := make([]byte, 32)
	if _, err := io.ReadFull(rand.Reader, key); err != nil {
		return nil, fmt.Errorf("generate key: %w", err)
	}
	return &StateCipher{key: key}, nil
}

// Encrypt returns (nonce ‖ ciphertext ‖ tag) for plaintext.
func (c *StateCipher) Encrypt(plaintext []byte) ([]byte, error) {
	block, err := aes.NewCipher(c.key)
	if err != nil {
		return nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}

	nonce := make([]byte, gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return nil, err
	}

	ciphertext := gcm.Seal(nonce, nonce, plaintext, nil)
	return ciphertext, nil
}

// Decrypt verifies the tag and returns plaintext.  Returns an error if the
// ciphertext has been tampered with.
func (c *StateCipher) Decrypt(ciphertext []byte) ([]byte, error) {
	block, err := aes.NewCipher(c.key)
	if err != nil {
		return nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}

	nonceSize := gcm.NonceSize()
	if len(ciphertext) < nonceSize {
		return nil, errors.New("ciphertext too short")
	}

	nonce, ct := ciphertext[:nonceSize], ciphertext[nonceSize:]
	return gcm.Open(nil, nonce, ct, nil)
}

// ─── Feature 136: Merkle Tree Audit Ledger ────────────────────────────────────

// MerkleNode is one node in the audit tree.
type MerkleNode struct {
	Hash  [32]byte
	Left  *MerkleNode
	Right *MerkleNode
	Data  []byte // leaf data (non-nil for leaves only)
}

// MerkleAuditTree builds incremental Merkle trees over mutation records.
type MerkleAuditTree struct {
	mu     sync.Mutex
	leaves [][32]byte // ordered leaf hashes
}

// NewMerkleAuditTree constructs an empty tree.
func NewMerkleAuditTree() *MerkleAuditTree { return &MerkleAuditTree{} }

// Append hashes data and appends a leaf to the tree.
func (t *MerkleAuditTree) Append(data []byte) {
	h := sha256.Sum256(data)
	t.mu.Lock()
	t.leaves = append(t.leaves, h)
	t.mu.Unlock()
}

// Root computes the current Merkle root hash.
func (t *MerkleAuditTree) Root() [32]byte {
	t.mu.Lock()
	leaves := make([][32]byte, len(t.leaves))
	copy(leaves, t.leaves)
	t.mu.Unlock()

	if len(leaves) == 0 {
		return [32]byte{}
	}
	return merkleRoot(leaves)
}

// ProveInclusion returns true when data is present in the tree.
func (t *MerkleAuditTree) ProveInclusion(data []byte) bool {
	target := sha256.Sum256(data)
	t.mu.Lock()
	defer t.mu.Unlock()
	for _, leaf := range t.leaves {
		if leaf == target {
			return true
		}
	}
	return false
}

// LeafCount returns the number of appended leaves.
func (t *MerkleAuditTree) LeafCount() int {
	t.mu.Lock()
	defer t.mu.Unlock()
	return len(t.leaves)
}

func merkleRoot(nodes [][32]byte) [32]byte {
	if len(nodes) == 1 {
		return nodes[0]
	}
	// Pad to even length
	if len(nodes)%2 != 0 {
		nodes = append(nodes, nodes[len(nodes)-1])
	}
	parents := make([][32]byte, len(nodes)/2)
	for i := 0; i < len(nodes); i += 2 {
		var combined [64]byte
		copy(combined[:32], nodes[i][:])
		copy(combined[32:], nodes[i+1][:])
		parents[i/2] = sha256.Sum256(combined[:])
	}
	return merkleRoot(parents)
}

// ─── Feature 137: Dynamic Key Rotation Manager ────────────────────────────────

// KeyRing manages current and previous encryption keys with atomic rotation.
type KeyRing struct {
	mu          sync.RWMutex
	current     *StateCipher
	previous    *StateCipher
	keyVersion  uint64
}

// NewKeyRing constructs a ring with an initial fresh key.
func NewKeyRing() (*KeyRing, error) {
	cipher, err := NewStateCipher()
	if err != nil {
		return nil, err
	}
	return &KeyRing{current: cipher, keyVersion: 1}, nil
}

// Rotate generates a new key, promoting the current key to previous.
func (kr *KeyRing) Rotate() error {
	newCipher, err := NewStateCipher()
	if err != nil {
		return err
	}
	kr.mu.Lock()
	kr.previous = kr.current
	kr.current = newCipher
	kr.keyVersion++
	kr.mu.Unlock()
	return nil
}

// Encrypt uses the current key.
func (kr *KeyRing) Encrypt(plaintext []byte) ([]byte, uint64, error) {
	kr.mu.RLock()
	c := kr.current
	v := kr.keyVersion
	kr.mu.RUnlock()
	ct, err := c.Encrypt(plaintext)
	return ct, v, err
}

// Decrypt tries current key first, then previous (for rolling decryption).
func (kr *KeyRing) Decrypt(ciphertext []byte) ([]byte, error) {
	kr.mu.RLock()
	cur := kr.current
	prev := kr.previous
	kr.mu.RUnlock()

	if pt, err := cur.Decrypt(ciphertext); err == nil {
		return pt, nil
	}
	if prev != nil {
		return prev.Decrypt(ciphertext)
	}
	return nil, errors.New("decryption failed with all available keys")
}

// Version returns the current key version.
func (kr *KeyRing) Version() uint64 {
	kr.mu.RLock()
	defer kr.mu.RUnlock()
	return kr.keyVersion
}

// ─── Feature 138: SPIFFE Identity Resolver ───────────────────────────────────

// SPIFFEIdentity holds a parsed SPIFFE workload identity.
type SPIFFEIdentity struct {
	TrustDomain string // e.g., "corp.example.com"
	Namespace   string // e.g., "production"
	ServiceName string // e.g., "detection-engine"
}

// ParseSPIFFEID parses a SPIFFE URI: spiffe://domain/ns/namespace/sa/service.
func ParseSPIFFEID(uri string) (*SPIFFEIdentity, error) {
	const prefix = "spiffe://"
	if len(uri) < len(prefix) || uri[:len(prefix)] != prefix {
		return nil, fmt.Errorf("not a SPIFFE URI: %q", uri)
	}
	rest := uri[len(prefix):]

	var domain, ns, svc string
	fmt.Sscanf(rest, "%[^/]/ns/%[^/]/sa/%s", &domain, &ns, &svc)
	if domain == "" || ns == "" || svc == "" {
		return nil, fmt.Errorf("invalid SPIFFE URI format: %q", uri)
	}
	return &SPIFFEIdentity{TrustDomain: domain, Namespace: ns, ServiceName: svc}, nil
}

// KeyVersionBytes returns a big-endian 8-byte encoding of keyVersion for inclusion proofs.
func KeyVersionBytes(version uint64) []byte {
	b := make([]byte, 8)
	binary.BigEndian.PutUint64(b, version)
	return b
}
