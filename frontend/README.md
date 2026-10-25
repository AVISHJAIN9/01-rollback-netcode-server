# Frontend: Netcode Visualizer & Interactive Arena Client

## Technology Stack
- **Framework**: Next.js 14 (App Router) + TypeScript
- **Styling**: TailwindCSS with dark futuristic cyberpunk theme
- **Animations**: GSAP ScrollTrigger + Lenis smooth scrolling
- **Graphics**: Three.js / Canvas 2D for 60 FPS arena rendering and state visualization

## Routes
- `/`: 3D landing page with scroll-driven arena sequence
- `/dashboard`: Real-time system internals dashboard (ring buffer gauge, RTT jitter timeseries, rollback frequency)
- `/arena`: Playable 2D arena with network impairment simulation
- `/ai-insights`: AI diagnostics studio (cheat detection plots, LLM desync root-cause explainer)

## Local Development
```bash
npm install
npm run dev
```
