import { WorldState } from "./state";

export class SnapshotRingBuffer {
  public static readonly CAPACITY = 128;
  private states: (WorldState | null)[];
  private checksums: string[];
  public headFrame: number;

  constructor() {
    this.states = new Array(SnapshotRingBuffer.CAPACITY).fill(null);
    this.checksums = new Array(SnapshotRingBuffer.CAPACITY).fill("");
    this.headFrame = 0;
  }

  private getSlotIndex(frame: number): number {
    return frame & (SnapshotRingBuffer.CAPACITY - 1);
  }

  public save(frame: number, state: WorldState, checksum: string): void {
    const slot = this.getSlotIndex(frame);
    this.states[slot] = state.clone();
    this.checksums[slot] = checksum;
    if (frame > this.headFrame) {
      this.headFrame = frame;
    }
  }

  public get(frame: number): WorldState | null {
    if (frame + SnapshotRingBuffer.CAPACITY <= this.headFrame || frame > this.headFrame) {
      return null; // Expired or future frame
    }
    const slot = this.getSlotIndex(frame);
    const s = this.states[slot];
    return s && s.frameIndex === frame ? s.clone() : null;
  }

  public getChecksum(frame: number): string | null {
    if (frame + SnapshotRingBuffer.CAPACITY <= this.headFrame || frame > this.headFrame) {
      return null;
    }
    return this.checksums[this.getSlotIndex(frame)];
  }
}
