import { FixedI32, Vec2Fixed } from "../math/fixed_point";

export class ServerAuthorityValidator {
  public static readonly MAX_FEASIBLE_SPEED = FixedI32.fromFloat(15.0);

  /**
   * Validates if a proposed delta coordinate is physically feasible under arena gravity and max velocity.
   */
  public static validateKinematicTransition(
    currentPos: Vec2Fixed,
    proposedPos: Vec2Fixed,
    dtTicks: number = 1
  ): { valid: boolean; reason?: string } {
    const dx = proposedPos.x.sub(currentPos.x).abs();
    const dy = proposedPos.y.sub(currentPos.y).abs();
    const maxDelta = ServerAuthorityValidator.MAX_FEASIBLE_SPEED.mul(FixedI32.fromInt(dtTicks));

    if (dx.raw > maxDelta.raw) {
      return { valid: false, reason: "Horizontal teleportation / speed-hack delta exceeded limit" };
    }
    if (dy.raw > maxDelta.raw) {
      return { valid: false, reason: "Vertical teleportation delta exceeded limit" };
    }

    return { valid: true };
  }
}
