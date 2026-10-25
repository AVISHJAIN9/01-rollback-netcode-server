import Link from 'next/link'
import { Activity, Play, ShieldAlert, Cpu, Sparkles } from 'lucide-react'

export function Navbar() {
  return (
    <header className="sticky top-0 z-50 backdrop-blur-md bg-background/80 border-b border-slate-800">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-16 flex items-center justify-between">
        <Link href="/" className="flex items-center space-x-3">
          <div className="w-8 h-8 rounded-lg bg-gradient-to-tr from-primary to-accent flex items-center justify-center font-bold text-black text-sm">
            RN
          </div>
          <span className="font-bold text-lg tracking-wide text-white">Rollback<span className="text-primary">Netcode</span></span>
        </Link>
        <nav className="hidden md:flex items-center space-x-6 text-sm font-medium">
          <Link href="/dashboard" className="text-slate-300 hover:text-primary transition-colors flex items-center gap-1.5">
            <Activity className="w-4 h-4" /> Telemetry Dashboard
          </Link>
          <Link href="/arena" className="text-slate-300 hover:text-primary transition-colors flex items-center gap-1.5">
            <Play className="w-4 h-4" /> 2D Arena Client
          </Link>
          <Link href="/ai-insights" className="text-slate-300 hover:text-primary transition-colors flex items-center gap-1.5">
            <Sparkles className="w-4 h-4 text-accent" /> AI Diagnostics
          </Link>
        </nav>
        <div className="flex items-center space-x-3">
          <button className="px-4 py-2 rounded-lg bg-slate-800 hover:bg-slate-700 text-sm font-medium transition-all text-slate-200 border border-slate-700">
            Sign In with Google
          </button>
        </div>
      </div>
    </header>
  )
}
