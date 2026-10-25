import './globals.css'
import type { Metadata } from 'next'
import { Navbar } from './components/Navbar'

export const metadata: Metadata = {
  title: 'Rollback Netcode Engine | Deterministic 2D Arena & Systems Telemetry',
  description: 'Server-authoritative multiplayer netcode with sub-millisecond rollback resimulation, fixed-point determinism, and AI operations.',
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="en" className="dark">
      <body className="bg-background text-slate-100 min-h-screen antialiased flex flex-col font-sans">
        <Navbar />
        <main className="flex-1">
          {children}
        </main>
      </body>
    </html>
  )
}
