'use client'

import React, { useState } from 'react'
import { ShieldAlert, Ban, UserX, AlertCircle, RefreshCw } from 'lucide-react'

export default function AdminModerationPage() {
  const [sessions] = useState([
    { id: 'sess_9921a', room: 'ARENA-01', players: 2, region: 'us-east', status: 'ACTIVE', tickHealth: '100%' },
    { id: 'sess_9922b', room: 'ARENA-02', players: 2, region: 'eu-central', status: 'ACTIVE', tickHealth: '99.8%' },
    { id: 'sess_9923c', room: 'ARENA-03', players: 1, region: 'us-west', status: 'LOBBY', tickHealth: '100%' },
  ])

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
      <div className="mb-8">
        <div className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-red-500/10 border border-red-500/20 text-xs font-mono text-red-400 mb-2">
          <ShieldAlert className="w-3.5 h-3.5" /> ROLE-BASED ADMIN & MODERATION CONTROL
        </div>
        <h1 className="text-3xl font-bold text-white mb-2">Game Operations & Incident Control</h1>
        <p className="text-slate-400 text-sm">Real-time room lifecycle management, cheat investigation, player bans, and session termination.</p>
      </div>

      {/* Active Sessions Table */}
      <div className="p-6 rounded-2xl bg-slate-900 border border-slate-800 mb-8">
        <h3 className="text-lg font-bold text-white mb-4 flex items-center gap-2">
          <RefreshCw className="w-5 h-5 text-cyan-400" /> Active Multiplayer Match Sessions
        </h3>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm font-mono">
            <thead>
              <tr className="border-b border-slate-800 text-slate-500 text-xs">
                <th className="pb-3">SESSION ID</th>
                <th className="pb-3">ROOM</th>
                <th className="pb-3">PLAYERS</th>
                <th className="pb-3">REGION</th>
                <th className="pb-3">STATUS</th>
                <th className="pb-3">TICK HEALTH</th>
                <th className="pb-3 text-right">ACTION</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-800/60">
              {sessions.map((s) => (
                <tr key={s.id} className="text-slate-300">
                  <td className="py-3 text-cyan-400">{s.id}</td>
                  <td className="py-3">{s.room}</td>
                  <td className="py-3">{s.players}/2</td>
                  <td className="py-3">{s.region}</td>
                  <td className="py-3"><span className="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-xs">{s.status}</span></td>
                  <td className="py-3 text-emerald-400">{s.tickHealth}</td>
                  <td className="py-3 text-right">
                    <button className="px-3 py-1 rounded bg-red-500/10 hover:bg-red-500/20 text-red-400 text-xs border border-red-500/20">
                      Terminate Room
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}
