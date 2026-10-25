export class TokenBucketRateLimiter {
  private capacity: number;
  private refillRatePerSec: number;
  private tokens: Map<string, { count: number; lastRefill: number }> = new Map();

  constructor(capacity: number = 60, refillRatePerSec: number = 60) {
    this.capacity = capacity;
    this.refillRatePerSec = refillRatePerSec;
  }

  public allowRequest(key: string): boolean {
    const now = Date.now();
    let bucket = this.tokens.get(key);

    if (!bucket) {
      bucket = { count: this.capacity - 1, lastRefill: now };
      this.tokens.set(key, bucket);
      return true;
    }

    // Refill tokens based on elapsed time
    const elapsedSec = (now - bucket.lastRefill) / 1000.0;
    bucket.count = Math.min(this.capacity, bucket.count + elapsedSec * this.refillRatePerSec);
    bucket.lastRefill = now;

    if (bucket.count >= 1.0) {
      bucket.count -= 1.0;
      return true;
    }

    return false; // Throttled!
  }
}
