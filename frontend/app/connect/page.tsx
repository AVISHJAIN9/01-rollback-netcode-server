'use client'

import React, { useState } from 'react'
import { Wifi, Globe, Shield, CheckCircle, ArrowRight } from 'lucide-react'
import Link from 'next/link'

export default function ConnectionSetupPage() {
  const [selectedRegion, setSelectedRegion] = useState('us-east')
  const [protocol, setProtocol] = useState<'udp' | 'ws'>('udp')

  const regions = [
    { id: 'us-east', name: 'US East (N. Virginia)', ping: 18 },
    { id: 'us-west', name: 'US West (Oregon)', ping: 42 },
    { id: 'eu-central', name: 'EU Central (Frankfurt)', ping: 88 },
    { id: 'ap-south', name: 'Asia Pacific (Mumbai)', ping: 110 },
  ]

  return (
    <div className="max-w-4xl mx-auto px-4 py-12">
      <div className="mb-8">
        <h1 className="text-3xl font-bold text-white mb-2">Connection & Region Setup</h1>
        <p className="text-slate-400 text-sm">Select optimal game server region and transport protocol for low-latency rollback netcode.</p>
      </div>

      <div className="space-y-6">
        {/* Region Selector */}
        <div className="p-6 rounded-2xl bg-slate-900 border border-slate-800 space-y-4">
          <h3 className="text-sm font-bold text-white uppercase tracking-wider flex items-center gap-2">
            <Globe className="w-4 h-4 text-cyan-400" /> Available Regions
          </h3>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            {regions.map((r) => (
              <div
                key={r.id}
                onClick={() => setSelectedRegion(r.id)}
                className={`p-4 rounded-xl border cursor-pointer transition-all flex justify-between items-center ${
                  selectedRegion === r.id
                    ? 'bg-cyan-500/10 border-cyan-400 text-white shadow-lg shadow-cyan-400/10'
                    : 'bg-slate-950 border-slate-800 text-slate-400 hover:border-slate-700'
                }`}
              >
                <div>
                  <div className="font-semibold text-sm">{r.name}</div>
                  <div className="text-xs text-slate-500 mt-0.5 font-mono">{r.id}</div>
                </div>
                <div className="text-xs font-mono font-bold text-emerald-400">
                  {r.ping} ms
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Transport Mode */}
        <div className="p-6 rounded-2xl bg-slate-900 border border-slate-800 space-y-4">
          <h3 className="text-sm font-bold text-white uppercase tracking-wider flex items-center gap-2">
            <Wifi className="w-4 h-4 text-pink-400" /> Transport Protocol
          </h3>
          <div className="grid grid-cols-2 gap-4 font-mono text-xs">
            <button
              onClick={() => setProtocol('udp')}
              className={`p-4 rounded-xl border text-left ${protocol === 'udp' ? 'bg-cyan-500/10 border-cyan-400 text-white' : 'bg-slate-950 border-slate-800 text-slate-400'}`}
            >
              <div className="font-bold text-sm text-cyan-400">UDP Datagrams (Recommended)</div>
              <div className="text-slate-400 mt-1">5-Frame Redundant Input Bundling</div>
            </button>
            <button
              onClick={() => setProtocol('ws')}
              className={`p-4 rounded-xl border text-left ${protocol === 'ws' ? 'bg-pink-500/10 border-pink-400 text-white' : 'bg-slate-950 border-slate-800 text-slate-400'}`}
            >
              <div className="font-bold text-sm text-pink-400">Binary WebSocket</div>
              <div className="text-slate-400 mt-1">Fallback for Restrictive Firewalls</div>
            </button>
          </div>
        </div>

        <div className="flex justify-end">
          <Link
            href="/lobby"
            className="px-8 py-3.5 rounded-xl bg-cyan-400 hover:bg-cyan-300 text-black font-semibold text-sm flex items-center gap-2 shadow-lg shadow-cyan-400/20"
          >
            Enter Match Lobby <ArrowRight className="w-4 h-4" />
          </Link>
        </div>
      </div>
    </div>
  )
}
