package phase17

import (
	"sync/atomic"
)

// ─── Feature 99: Chase-Lev Lock-Free Work-Stealing Scheduler ─────────────────
//
// Each worker has a circular array deque.  The owning worker pushes and pops
// from the bottom (LIFO for cache locality).  Idle workers steal from the top
// of a victim's deque via a single CAS (FIFO for fairness).
//
// Reference: "Dynamic Circular Work-Stealing Deque" — Chase & Lev, 2005.

const initialDequeSize = 256

// task is the unit of work stored in the deque.
type task struct {
	id    int
	value interface{}
}

// StealResult is the outcome of a steal attempt.
type StealResult int

const (
	StealSuccess StealResult = iota
	StealEmpty               // victim deque was empty
	StealAbort               // CAS failed (race with concurrent steal or owner pop)
)

// workerDeque is a single-owner work-stealing deque backed by a circular array.
type workerDeque struct {
	top    atomic.Int64  // stolen from here (CAS by stealers)
	_pad0  [56]byte
	bottom atomic.Int64  // owned by local worker (no CAS needed)
	_pad1  [56]byte
	buf    []atomic.Pointer[task]
	size   int
}

func newWorkerDeque(capacity int) *workerDeque {
	d := &workerDeque{size: capacity}
	d.buf = make([]atomic.Pointer[task], capacity)
	return d
}

func (d *workerDeque) slot(idx int64) *atomic.Pointer[task] {
	return &d.buf[int(idx)%d.size]
}

// PushLocal adds a task to the bottom (local push — no CAS).
func (d *workerDeque) PushLocal(t *task) {
	b := d.bottom.Load()
	d.slot(b).Store(t)
	// Publish bottom increment after storing the task.
	d.bottom.Add(1)
}

// PopLocal removes a task from the bottom (local pop — CAS only on conflict).
func (d *workerDeque) PopLocal() (*task, bool) {
	b := d.bottom.Load() - 1
	d.bottom.Store(b)

	top := d.top.Load()
	if top <= b {
		t := d.slot(b).Load()
		if top == b {
			// Last item: compete with potential stealers
			if !d.top.CompareAndSwap(top, top+1) {
				// Stealer won — deque is now empty
				d.bottom.Store(b + 1)
				return nil, false
			}
			d.bottom.Store(b + 1)
		}
		return t, true
	}
	// Deque was already empty
	d.bottom.Store(b + 1)
	return nil, false
}

// StealFrom attempts to steal one task from the top of another worker's deque.
func (d *workerDeque) StealFrom(victim *workerDeque) (*task, StealResult) {
	top := victim.top.Load()
	// Ensure we read top before bottom (acquire semantics).
	bot := victim.bottom.Load()

	if top >= bot {
		return nil, StealEmpty
	}

	t := victim.slot(top).Load()
	if t == nil {
		return nil, StealAbort
	}

	// CAS top to claim the slot.
	if !victim.top.CompareAndSwap(top, top+1) {
		return nil, StealAbort
	}

	return t, StealSuccess
}

// Len returns the approximate current deque size (not exact under concurrency).
func (d *workerDeque) Len() int {
	b := d.bottom.Load()
	t := d.top.Load()
	if b > t {
		return int(b - t)
	}
	return 0
}

// WorkStealingScheduler manages N per-worker deques.
type WorkStealingScheduler struct {
	workers []*workerDeque
	n       int
	nextID  atomic.Int64
}

// NewWorkStealingScheduler constructs a scheduler with n workers.
func NewWorkStealingScheduler(n int) *WorkStealingScheduler {
	ws := &WorkStealingScheduler{
		workers: make([]*workerDeque, n),
		n:       n,
	}
	for i := 0; i < n; i++ {
		ws.workers[i] = newWorkerDeque(initialDequeSize)
	}
	return ws
}

// Push adds a task to workerID's local deque.
func (ws *WorkStealingScheduler) Push(workerID int, value interface{}) {
	id := int(ws.nextID.Add(1))
	ws.workers[workerID].PushLocal(&task{id: id, value: value})
}

// Pop removes a task from workerID's own deque.
func (ws *WorkStealingScheduler) Pop(workerID int) (interface{}, bool) {
	t, ok := ws.workers[workerID].PopLocal()
	if !ok {
		return nil, false
	}
	return t.value, true
}

// Steal attempts to steal one task from victimID's deque.
func (ws *WorkStealingScheduler) Steal(thiefID, victimID int) (interface{}, StealResult) {
	t, result := ws.workers[thiefID].StealFrom(ws.workers[victimID])
	if result != StealSuccess {
		return nil, result
	}
	return t.value, StealSuccess
}

// WorkerDepth returns the approximate number of tasks in workerID's deque.
func (ws *WorkStealingScheduler) WorkerDepth(workerID int) int {
	return ws.workers[workerID].Len()
}
