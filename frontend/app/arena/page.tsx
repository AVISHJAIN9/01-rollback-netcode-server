'use client'

import React, { useEffect, useRef, useState } from 'react'
import { FixedI32, Vec2Fixed } from '../../../shared/math/fixed_point'
import { WorldState } from '../../../shared/simulation/state'
import { RollbackEngine } from '../../../shared/simulation/rollback'
import { InputButtons, FrameInput } from '../../../shared/protocol/packets'
import { Play, Pause, RefreshCw, Zap, Shield, Wifi, Sliders, AlertTriangle } from 'lucide-react'

export default function ArenaPlaygroundPage() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const [isPlaying, setIsPlaying] = useState(true)
  const [latencyMs, setLatencyMs] = useState(50)
  const [jitterMs, setJitterMs] = useState(15)
  const [packetLossPct, setPacketLossPct] = useState(5)
  
  // Real-time telemetry display
  const [fps, setFps] = useState(60)
  const [currentFrame, setCurrentFrame] = useState(0)
  const [rollbackCount, setRollbackCount] = useState(0)
  const [lastRollbackFrames, setLastRollbackFrames] = useState(0)
  const [p1Health, setP1Health] = useState(100)
  const [p2Health, setP2Health] = useState(100)
  const [checksum, setChecksum] = useState('811C9DC5')

  // Engine references
  const engineRef = useRef<RollbackEngine>(new RollbackEngine(0))
  const keyStateRef = useRef<{ [key: string]: boolean }>({})
  const packetQueueRef = useRef<{ deliverAt: number; frame: number; input: FrameInput }[]>([])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      keyStateRef.current[e.code] = true
    }
    const handleKeyUp = (e: KeyboardEvent) => {
      keyStateRef.current[e.code] = false
    }

    window.addEventListener('keydown', handleKeyDown)
    window.addEventListener('keyup', handleKeyUp)

    let animationId: number
    let lastTime = performance.now()
    let frameAccumulator = 0
    const TICK_INTERVAL = 1000 / 60

    const engine = engineRef.current

    const gameLoop = (currentTime: number) => {
      const delta = currentTime - lastTime
      lastTime = currentTime
      frameAccumulator += delta

      while (frameAccumulator >= TICK_INTERVAL) {
        frameAccumulator -= TICK_INTERVAL

        if (isPlaying) {
          // 1. Capture Local Player 0 Inputs
          let buttons = 0
          if (keyStateRef.current['ArrowLeft'] || keyStateRef.current['KeyA']) buttons |= InputButtons.LEFT
          if (keyStateRef.current['ArrowRight'] || keyStateRef.current['KeyD']) buttons |= InputButtons.RIGHT
          if (keyStateRef.current['ArrowUp'] || keyStateRef.current['KeyW'] || keyStateRef.current['Space']) buttons |= InputButtons.JUMP
          if (keyStateRef.current['KeyJ'] || keyStateRef.current['KeyF']) buttons |= InputButtons.ATTACK

          const localInput: FrameInput = {
            frame: engine.activeState.frameIndex,
            buttons,
            analogX: 0,
            analogY: 0,
          }

          // 2. Advance Local Simulation with Prediction
          const activeState = engine.advanceFrame(localInput)

          // 3. Simulate Remote Player (Bot AI) with Network Impairment
          const botButtons = (Math.sin(activeState.frameIndex * 0.05) > 0.3 ? InputButtons.LEFT : InputButtons.RIGHT) |
                             (Math.random() < 0.03 ? InputButtons.JUMP : 0) |
                             (Math.random() < 0.05 ? InputButtons.ATTACK : 0)

          const remoteInput: FrameInput = {
            frame: engine.activeState.frameIndex,
            buttons: botButtons,
            analogX: 0,
            analogY: 0,
          }

          // Simulate packet latency + jitter
          const actualLatency = Math.max(5, latencyMs + (Math.random() * 2 - 1) * jitterMs)
          const isLost = Math.random() * 100 < packetLossPct

          if (!isLost) {
            packetQueueRef.current.push({
              deliverAt: currentTime + actualLatency,
              frame: engine.activeState.frameIndex,
              input: remoteInput,
            })
          }

          // 4. Deliver Delayed Packets & Trigger Rollback if needed
          const remainingPackets = []
          for (const pkt of packetQueueRef.current) {
            if (currentTime >= pkt.deliverAt) {
              engine.handleRemoteInput(pkt.frame, 1, pkt.input)
            } else {
              remainingPackets.push(pkt)
            }
          }
          packetQueueRef.current = remainingPackets

          // Update metrics
          setCurrentFrame(engine.activeState.frameIndex)
          setRollbackCount(engine.metrics.totalRollbacks)
          setLastRollbackFrames(engine.metrics.lastRollbackFrames)
          setP1Health(engine.activeState.players[0].health)
          setP2Health(engine.activeState.players[1].health)
          setChecksum(engine.activeState.computeChecksum())
        }
      }

      // 5. Render Canvas Viewport
      const canvas = canvasRef.current
      if (canvas) {
        const ctx = canvas.getContext('2d')
        if (ctx) {
          ctx.clearRect(0, 0, canvas.width, canvas.height)

          // Draw Background Grid
          ctx.strokeStyle = '#1E293B'
          ctx.lineWidth = 1
          for (let x = 0; x < canvas.width; x += 40) {
            ctx.beginPath()
            ctx.moveTo(x, 0)
            ctx.lineTo(x, canvas.height)
            ctx.stroke()
          }
          for (let y = 0; y < canvas.height; y += 40) {
            ctx.beginPath()
            ctx.moveTo(0, y)
            ctx.lineTo(canvas.width, y)
            ctx.stroke()
          }

          // Draw Arena Floor
          ctx.fillStyle = '#334155'
          ctx.fillRect(0, 420, canvas.width, canvas.height - 420)
          ctx.fillStyle = '#00F5FF'
          ctx.fillRect(0, 418, canvas.width, 2)

          // Draw Players
          const players = engine.activeState.players
          players.forEach((p) => {
            const px = p.position.x.toFloat()
            const py = p.position.y.toFloat()

            // Shadow
            ctx.fillStyle = 'rgba(0, 0, 0, 0.4)'
            ctx.beginPath()
            ctx.ellipse(px, 420, 20, 6, 0, 0, Math.PI * 2)
            ctx.fill()

            // Body
            ctx.fillStyle = p.id === 0 ? '#00F5FF' : '#FF0080'
            ctx.beginPath()
            ctx.roundRect(px - 16, py - 32, 32, 32, 6)
            ctx.fill()

            // Direction Eye
            ctx.fillStyle = '#FFFFFF'
            const eyeX = p.facingLeft ? px - 10 : px + 6
            ctx.fillRect(eyeX, py - 24, 4, 6)

            // Attack Slash Effect
            if (p.isAttacking) {
              ctx.strokeStyle = p.id === 0 ? '#00F5FF' : '#FF0080'
              ctx.lineWidth = 3
              ctx.beginPath()
              const slashX = p.facingLeft ? px - 28 : px + 28
              ctx.arc(slashX, py - 16, 20, p.facingLeft ? Math.PI * 0.7 : -Math.PI * 0.3, p.facingLeft ? Math.PI * 1.3 : Math.PI * 0.3)
              ctx.stroke()
            }

            // Health bar above player
            ctx.fillStyle = '#1E293B'
            ctx.fillRect(px - 20, py - 46, 40, 6)
            ctx.fillStyle = p.id === 0 ? '#00E676' : '#FFB800'
            ctx.fillRect(px - 20, py - 46, (p.health / 100) * 40, 6)
          })
        }
      }

      animationId = requestAnimationFrame(gameLoop)
    }

    animationId = requestAnimationFrame(gameLoop)

    return () => {
      window.removeEventListener('keydown', handleKeyDown)
      window.removeEventListener('keyup', handleKeyUp)
      cancelAnimationFrame(animationId)
    }
  }, [isPlaying, latencyMs, jitterMs, packetLossPct])

  const handleReset = () => {
    engineRef.current = new RollbackEngine(0)
    packetQueueRef.current = []
  }

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
      {/* Header */}
      <div className="flex flex-col md:flex-row justify-between items-start md:items-center mb-6 gap-4">
        <div>
          <div className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-cyan-500/10 border border-cyan-500/20 text-xs font-mono text-cyan-400 mb-2">
            <Zap className="w-3.5 h-3.5" /> DETERMINISTIC 60 FPS ROLLBACK SIMULATOR
          </div>
          <h1 className="text-3xl font-bold text-white">Interactive 2D Arena Testbed</h1>
        </div>
        <div className="flex gap-3">
          <button
            onClick={() => setIsPlaying(!isPlaying)}
            className="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-white font-medium text-sm border border-slate-700 flex items-center gap-2"
          >
            {isPlaying ? <Pause className="w-4 h-4" /> : <Play className="w-4 h-4" />}
            {isPlaying ? 'Pause' : 'Resume'}
          </button>
          <button
            onClick={handleReset}
            className="px-4 py-2 rounded-xl bg-cyan-400 hover:bg-cyan-300 text-black font-semibold text-sm flex items-center gap-2 shadow-lg shadow-cyan-400/20"
          >
            <RefreshCw className="w-4 h-4" /> Reset Match
          </button>
        </div>
      </div>

      {/* Main Game & Telemetry Layout */}
      <div className="grid grid-cols-1 lg:grid-cols-4 gap-6">
        {/* Canvas Viewport */}
        <div className="lg:col-span-3 space-y-4">
          <div className="relative rounded-2xl bg-slate-950 border border-slate-800 overflow-hidden shadow-2xl">
            {/* Top HUD Overlay */}
            <div className="absolute top-4 left-4 right-4 flex justify-between items-center text-xs font-mono z-10 bg-slate-900/80 backdrop-blur-md px-4 py-2 rounded-xl border border-slate-800">
              <div className="flex items-center gap-4">
                <span className="text-cyan-400 font-bold">P1 (Local): {p1Health} HP</span>
                <span className="text-slate-500">|</span>
                <span className="text-pink-400 font-bold">P2 (Remote Bot): {p2Health} HP</span>
              </div>
              <div className="flex items-center gap-4">
                <span className="text-slate-400">Frame: <strong className="text-white">#{currentFrame}</strong></span>
                <span className="text-slate-400">Hash: <strong className="text-emerald-400 font-mono">{checksum}</strong></span>
              </div>
            </div>

            <canvas
              ref={canvasRef}
              width={800}
              height={500}
              className="w-full h-auto block"
            />

            {/* Keyboard Guide Banner */}
            <div className="absolute bottom-3 left-3 right-3 flex justify-between items-center text-[11px] font-mono text-slate-400 bg-slate-900/60 px-3 py-1.5 rounded-lg border border-slate-800/80">
              <span>Controls: <strong>A / D</strong> Move, <strong>Space / W</strong> Jump, <strong>J</strong> Attack</span>
              <span className="text-cyan-400">60 Hz Q16.16 Kinematics</span>
            </div>
          </div>

          {/* Rollback & Ring Buffer Telemetry Timeline */}
          <div className="p-5 rounded-2xl bg-slate-900/90 border border-slate-800">
            <div className="flex justify-between items-center mb-3 text-xs font-mono text-slate-400">
              <span className="flex items-center gap-1.5"><RefreshCw className="w-3.5 h-3.5 text-cyan-400" /> SNAPSHOT RING BUFFER (128 FRAMES)</span>
              <span>Total Rollbacks: <strong className="text-cyan-400">{rollbackCount}</strong> | Last Rewind: <strong className="text-amber-400">{lastRollbackFrames} frames</strong></span>
            </div>
            <div className="grid grid-cols-16 sm:grid-cols-32 gap-1">
              {Array.from({ length: 64 }).map((_, idx) => {
                const isCurrent = (currentFrame & 63) === idx
                return (
                  <div
                    key={idx}
                    className={`h-6 rounded-sm text-[9px] font-mono font-bold flex items-center justify-center transition-all ${
                      isCurrent
                        ? 'bg-cyan-400 text-black shadow-lg shadow-cyan-400/50 scale-110 z-10'
                        : idx < (currentFrame & 63)
                        ? 'bg-emerald-500/80 text-black'
                        : 'bg-slate-800 text-slate-500'
                    }`}
                  >
                    {idx}
                  </div>
                )
              })}
            </div>
          </div>
        </div>

        {/* Network Impairment Controls Sidebar */}
        <div className="space-y-6">
          <div className="p-6 rounded-2xl bg-slate-900 border border-slate-800 space-y-6">
            <h3 className="text-sm font-bold text-white uppercase tracking-wider flex items-center gap-2">
              <Sliders className="w-4 h-4 text-cyan-400" /> Network Impairment
            </h3>

            {/* Latency Slider */}
            <div>
              <div className="flex justify-between text-xs font-mono mb-2">
                <span className="text-slate-400">Simulated Latency</span>
                <span className="text-cyan-400 font-bold">{latencyMs} ms</span>
              </div>
              <input
                type="range"
                min="0"
                max="300"
                value={latencyMs}
                onChange={(e) => setLatencyMs(Number(e.target.value))}
                className="w-full accent-cyan-400 cursor-pointer"
              />
            </div>

            {/* Jitter Slider */}
            <div>
              <div className="flex justify-between text-xs font-mono mb-2">
                <span className="text-slate-400">Jitter Variance (±)</span>
                <span className="text-pink-400 font-bold">{jitterMs} ms</span>
              </div>
              <input
                type="range"
                min="0"
                max="100"
                value={jitterMs}
                onChange={(e) => setJitterMs(Number(e.target.value))}
                className="w-full accent-pink-400 cursor-pointer"
              />
            </div>

            {/* Packet Loss Slider */}
            <div>
              <div className="flex justify-between text-xs font-mono mb-2">
                <span className="text-slate-400">Packet Loss Rate</span>
                <span className="text-amber-400 font-bold">{packetLossPct} %</span>
              </div>
              <input
                type="range"
                min="0"
                max="50"
                value={packetLossPct}
                onChange={(e) => setPacketLossPct(Number(e.target.value))}
                className="w-full accent-amber-400 cursor-pointer"
              />
            </div>
          </div>

          {/* Netcode Architecture Status Card */}
          <div className="p-5 rounded-2xl bg-slate-900/60 border border-slate-800 space-y-3 font-mono text-xs">
            <div className="text-slate-400 font-bold mb-1 uppercase tracking-wider">Active Mechanisms</div>
            <div className="flex items-center justify-between text-slate-300">
              <span>Arithmetic Mode:</span>
              <span className="text-cyan-400">Q16.16 Fixed-Point</span>
            </div>
            <div className="flex items-center justify-between text-slate-300">
              <span>Snapshot Memory:</span>
              <span className="text-emerald-400">0 B Alloc / Tick</span>
            </div>
            <div className="flex items-center justify-between text-slate-300">
              <span>UDP Bundling:</span>
              <span className="text-cyan-400">5-Frame Bitmask</span>
            </div>
            <div className="flex items-center justify-between text-slate-300">
              <span>Desync Guard:</span>
              <span className="text-emerald-400">Blake3 Hash Sync</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
