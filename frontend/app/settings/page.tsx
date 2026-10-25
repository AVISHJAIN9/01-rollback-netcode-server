'use client'

import React, { useState } from 'react'
import { Sliders, Keyboard, Eye, Check } from 'lucide-react'

export default function SettingsPage() {
  const [saved, setSaved] = useState(false)
  const [controls, setControls] = useState({
    moveLeft: 'KeyA',
    moveRight: 'KeyD',
    jump: 'Space',
    attack: 'KeyJ',
  })

  const handleSave = () => {
    localStorage.setItem('netcode_controls', JSON.stringify(controls))
    setSaved(true)
    setTimeout(() => setSaved(false), 2000)
  }

  return (
    <div className="max-w-4xl mx-auto px-4 py-12">
      <div className="mb-8">
        <h1 className="text-3xl font-bold text-white mb-2">Controls & Accessibility</h1>
        <p className="text-slate-400 text-sm">Configure keybindings, dead zones, and accessibility color modes.</p>
      </div>

      <div className="p-6 rounded-2xl bg-slate-900 border border-slate-800 space-y-6 mb-6">
        <h3 className="text-sm font-bold text-white uppercase tracking-wider flex items-center gap-2">
          <Keyboard className="w-4 h-4 text-cyan-400" /> Key Remapping
        </h3>
        <div className="grid grid-cols-2 gap-4 font-mono text-xs">
          <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 flex justify-between items-center">
            <span className="text-slate-400">Move Left:</span>
            <span className="text-cyan-400 font-bold">{controls.moveLeft}</span>
          </div>
          <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 flex justify-between items-center">
            <span className="text-slate-400">Move Right:</span>
            <span className="text-cyan-400 font-bold">{controls.moveRight}</span>
          </div>
          <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 flex justify-between items-center">
            <span className="text-slate-400">Jump:</span>
            <span className="text-cyan-400 font-bold">{controls.jump}</span>
          </div>
          <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 flex justify-between items-center">
            <span className="text-slate-400">Attack:</span>
            <span className="text-cyan-400 font-bold">{controls.attack}</span>
          </div>
        </div>

        <button
          onClick={handleSave}
          className="px-6 py-2.5 rounded-xl bg-cyan-400 hover:bg-cyan-300 text-black font-semibold text-xs flex items-center gap-2"
        >
          {saved ? <Check className="w-3.5 h-3.5" /> : null}
          {saved ? 'Saved Successfully' : 'Save Keybindings'}
        </button>
      </div>
    </div>
  )
}
