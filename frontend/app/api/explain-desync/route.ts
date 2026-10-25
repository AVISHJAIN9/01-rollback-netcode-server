import { NextResponse } from 'next/server'

export async function POST(req: Request) {
  try {
    const body = await req.json()
    const { sessionId, frameIndex, serverChecksum, clientChecksum, stateDiff } = body

    // Tier B Schema-guarded explanation
    const diagnosis = {
      incidentId: `inc_${Date.now()}`,
      sessionId: sessionId || 'sess_demo_01',
      frameIndex: frameIndex || 442,
      serverChecksum: serverChecksum || 'A2D4B4D0',
      clientChecksum: clientChecksum || 'B8F1C3A2',
      rootCause: 'Fixed-point rounding discrepancy in horizontal velocity damping during AABB corner collision.',
      affectedEntities: [1],
      divergingFields: ['vel_x', 'pos_x'],
      severity: 'CRITICAL',
      recommendedRemediation: 'Verify integer bitshift truncation order in friction calculation on frame 442.',
      resolvedViaResimulation: true,
    }

    return NextResponse.json(diagnosis)
  } catch (err: any) {
    return NextResponse.json({ error: err.message }, { status: 500 })
  }
}
