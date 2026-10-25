// +build !linux

package phase17

// setOSThreadAffinity is a no-op on non-Linux platforms.
// On Linux the real implementation (syscall.RawSyscall SYS_SCHED_SETAFFINITY)
// would be in a linux-specific file.
func setOSThreadAffinity(coreID int) error {
	return nil
}
