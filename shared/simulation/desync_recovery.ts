import { WorldState } from "./state";
import { SnapshotRingBuffer } from "./ring_buffer";

export interface DesyncRecord {
  frame: number;
  serverChecksum: string;
  clientChecksum: string;
  divergingFields: string[];
  recovered: boolean;
}

export class DesyncRecoveryController {
  public static detectDivergence(
    frame: number,
    localChecksum: string,
    authoritativeChecksum: string
  ): boolean {
    return localChecksum.toUpperCase() !== authoritativeChecksum.toUpperCase();
  }

  /**
   * Forces state recovery by injecting authoritative server snapshot into the local ring buffer
   * and resetting active state forward from the authoritative baseline.
   */
  public static recoverFromAuthoritativeState(
    ringBuffer: SnapshotRingBuffer,
    authoritativeState: WorldState
  ): { activeState: WorldState; record: DesyncRecord } {
    const frame = authoritativeState.frameIndex;
    const authChecksum = authoritativeState.computeChecksum();

    // 1. Overwrite ring buffer slot with authoritative state
    ringBuffer.save(frame, authoritativeState, authChecksum);

    // 2. Clone to active state
    const recoveredActiveState = authoritativeState.clone();

    const record: DesyncRecord = {
      frame,
      serverChecksum: authChecksum,
      clientChecksum: ringBuffer.getChecksum(frame) || "UNKNOWN",
      divergingFields: ["position", "velocity", "health"],
      recovered: true,
    };

    return { activeState: recoveredActiveState, record };
  }
}
