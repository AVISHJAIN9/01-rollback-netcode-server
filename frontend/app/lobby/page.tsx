'use client'

import React, { useState } from 'react'
import { Users, Copy, Check, Play, Shield } from 'lucide-react'
import Link from 'next/link'

export default function MatchLobbyPage() {
  const [roomCode] = useState('ARENA-7721')
  const [copied, setCopied] = useState(false)
  const [isReady, setIsReady] = useState(false)

  const handleCopy = () => {
    navigator.clipboard.writeText(roomCode)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className="max-w-4xl mx-auto px-4 py-12">
      <div className="mb-8">
        <h1 className="text-3xl font-bold text-white mb-2">Match Lobby</h1>
        <p className="text-slate-400 text-sm">Room Code: <code className="text-cyan-400 font-bold">{roomCode}</code></p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-8">
        {/* Player 1 Slot */}
        <div className="p-6 rounded-2xl bg-slate-900 border border-slate-800 space-y-4">
          <div className="flex justify-between items-center">
            <span className="text-xs font-mono text-slate-400">SLOT 01 (HOST)</span>
            <span className="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-xs font-mono">CONNECTED</span>
          </div>
          <div className="font-bold text-lg text-white">Player 1 (You)</div>
          <div className="text-xs text-slate-500 font-mono">MMR: 1450 | Ping: 18ms</div>
          <button
            onClick={() => setIsReady(!isReady)}
            className={`w-full py-2.5 rounded-xl font-semibold text-sm transition-all ${
              isReady ? 'bg-emerald-500 text-black' : 'bg-slate-800 text-slate-300 hover:bg-slate-700'
            }`}
          >
            {isReady ? 'Ready for Match!' : 'Click to Ready Up'}
          </button>
        </div>

        {/* Player 2 Slot */}
        <div className="p-6 rounded-2xl bg-slate-900 border border-slate-800 space-y-4">
          <div className="flex justify-between items-center">
            <span className="text-xs font-mono text-slate-400">SLOT 02</span>
            <span className="px-2 py-0.5 rounded bg-pink-500/10 text-pink-400 text-xs font-mono">CONNECTED</span>
          </div>
          <div className="font-bold text-lg text-white">Opponent Bot (AI Peer)</div>
          <div className="text-xs text-slate-500 font-mono">MMR: 1420 | Ping: 42ms</div>
          <div className="w-full py-2.5 rounded-xl bg-emerald-500/20 text-emerald-400 text-center text-sm font-semibold">
            Ready
          </div>
        </div>
      </div>

      <div className="flex justify-between items-center p-6 rounded-2xl bg-slate-900/60 border border-slate-800">
        <button
          onClick={handleCopy}
          className="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-mono flex items-center gap-2"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
          {copied ? 'Copied Room Link' : 'Copy Room Code'}
        </button>

        <Link
          href="/arena"
          className="px-8 py-3.5 rounded-xl bg-cyan-400 hover:bg-cyan-300 text-black font-semibold text-sm flex items-center gap-2 shadow-lg shadow-cyan-400/20"
        >
          <Play className="w-4 h-4" /> Start Rollback Match
        </Link>
      </div>
    </div>
  )
}
