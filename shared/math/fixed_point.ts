/**
 * Q16.16 Fixed-Point Signed Integer Arithmetic.
 * 16 integer bits, 16 fractional bits.
 * Eliminates IEEE 754 floating-point drift across platforms (x86_64, ARM64, WASM, V8).
 * Complies with FR-001.
 */
export class FixedI32 {
  public static readonly FRACTIONAL_BITS = 16;
  public static readonly SCALE = 1 << 16; // 65536
  public static readonly HALF = 1 << 15;
  public static readonly ONE = new FixedI32(1 << 16);
  public static readonly ZERO = new FixedI32(0);

  public readonly raw: number; // 32-bit signed integer

  constructor(raw: number) {
    this.raw = raw | 0; // Force 32-bit integer in JS engine
  }

  public static fromInt(v: number): FixedI32 {
    return new FixedI32((v << 16) | 0);
  }

  public static fromFloat(v: number): FixedI32 {
    return new FixedI32(Math.round(v * 65536) | 0);
  }

  public toInt(): number {
    return this.raw >> 16;
  }

  public toFloat(): number {
    return this.raw / 65536.0;
  }

  public add(rhs: FixedI32): FixedI32 {
    return new FixedI32((this.raw + rhs.raw) | 0);
  }

  public sub(rhs: FixedI32): FixedI32 {
    return new FixedI32((this.raw - rhs.raw) | 0);
  }

  public mul(rhs: FixedI32): FixedI32 {
    // intermediate 64-bit multiplication via BigInt or double precision integer
    const p = BigInt(this.raw) * BigInt(rhs.raw);
    return new FixedI32(Number(p >> 16n) | 0);
  }

  public div(rhs: FixedI32): FixedI32 {
    if (rhs.raw === 0) throw new Error("FixedI32 division by zero");
    const d = (BigInt(this.raw) << 16n) / BigInt(rhs.raw);
    return new FixedI32(Number(d) | 0);
  }

  public abs(): FixedI32 {
    return this.raw < 0 ? new FixedI32((-this.raw) | 0) : this;
  }

  public clamp(min: FixedI32, max: FixedI32): FixedI32 {
    if (this.raw < min.raw) return min;
    if (this.raw > max.raw) return max;
    return this;
  }
}

export class Vec2Fixed {
  public x: FixedI32;
  public y: FixedI32;

  constructor(x: FixedI32, y: FixedI32) {
    this.x = x;
    this.y = y;
  }

  public static fromFloats(x: number, y: number): Vec2Fixed {
    return new Vec2Fixed(FixedI32.fromFloat(x), FixedI32.fromFloat(y));
  }

  public add(rhs: Vec2Fixed): Vec2Fixed {
    return new Vec2Fixed(this.x.add(rhs.x), this.y.add(rhs.y));
  }

  public sub(rhs: Vec2Fixed): Vec2Fixed {
    return new Vec2Fixed(this.x.sub(rhs.x), this.y.sub(rhs.y));
  }

  public mulScalar(rhs: FixedI32): Vec2Fixed {
    return new Vec2Fixed(this.x.mul(rhs), this.y.mul(rhs));
  }

  public clone(): Vec2Fixed {
    return new Vec2Fixed(new FixedI32(this.x.raw), new FixedI32(this.y.raw));
  }
}
