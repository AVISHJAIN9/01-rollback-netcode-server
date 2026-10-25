import Link from 'next/link'
import { Zap, ShieldCheck, RefreshCw, Cpu, Radio, ChevronRight } from 'lucide-react'

export default function LandingPage() {
  return (
    <div className="relative overflow-hidden">
      {/* Hero Section */}
      <section className="relative pt-24 pb-20 px-4 sm:px-6 lg:px-8 max-w-7xl mx-auto text-center">
        <div className="inline-flex items-center gap-2 px-3 py-1.5 rounded-full bg-surface border border-slate-800 text-xs font-mono text-primary mb-8">
          <Radio className="w-3.5 h-3.5 animate-pulse text-success" />
          <span>SERVER-AUTHORITATIVE ROLLBACK RESIMULATION ENGINE</span>
        </div>
        <h1 className="text-5xl sm:text-7xl font-extrabold tracking-tight text-white mb-6">
          Zero Perceived Lag. <br />
          <span className="bg-clip-text text-transparent bg-gradient-to-r from-primary via-purple-400 to-accent">
            Pure Deterministic State.
          </span>
        </h1>
        <p className="max-w-2xl mx-auto text-lg sm:text-xl text-slate-400 mb-10">
          Industrial-grade rollback netcode with Q16.16 fixed-point math, 128-frame circular ring buffers, Blake3 state checksumming, and real-time AI anomaly detection.
        </p>
        <div className="flex flex-wrap justify-center gap-4">
          <Link href="/arena" className="px-8 py-3.5 rounded-xl bg-primary hover:bg-primary/90 text-black font-semibold transition-all flex items-center gap-2 shadow-lg shadow-primary/20">
            Launch Arena Simulator <ChevronRight className="w-4 h-4" />
          </Link>
          <Link href="/dashboard" className="px-8 py-3.5 rounded-xl bg-surface hover:bg-slate-800 text-white font-semibold transition-all border border-slate-800">
            View Live Telemetry
          </Link>
        </div>
      </section>

      {/* Feature Grid */}
      <section className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-16">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
          <div className="p-8 rounded-2xl bg-surface/50 border border-slate-800/80 backdrop-blur-sm">
            <RefreshCw className="w-8 h-8 text-primary mb-4" />
            <h3 className="text-xl font-bold text-white mb-2">Sub-ms Resimulation</h3>
            <p className="text-slate-400 text-sm leading-relaxed">
              Rewind up to 128 frames and resimulate state trajectories in microseconds without heap allocations or garbage collection stalls.
            </p>
          </div>
          <div className="p-8 rounded-2xl bg-surface/50 border border-slate-800/80 backdrop-blur-sm">
            <Cpu className="w-8 h-8 text-secondary mb-4" />
            <h3 className="text-xl font-bold text-white mb-2">Q16.16 Determinism</h3>
            <p className="text-slate-400 text-sm leading-relaxed">
              Eliminates IEEE 754 floating-point drift across x86, ARM64, and WebAssembly with bit-exact fixed-point kinematics.
            </p>
          </div>
          <div className="p-8 rounded-2xl bg-surface/50 border border-slate-800/80 backdrop-blur-sm">
            <ShieldCheck className="w-8 h-8 text-accent mb-4" />
            <h3 className="text-xl font-bold text-white mb-2">AI Operations</h3>
            <p className="text-slate-400 text-sm leading-relaxed">
              Integrated Isolation Forest cheat detection, Kalman jitter prediction, and LLM-powered forensic desync explanations.
            </p>
          </div>
        </div>
      </section>
    </div>
  )
}
