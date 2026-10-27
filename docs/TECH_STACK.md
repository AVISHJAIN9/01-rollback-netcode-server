# Technology Stack Specification: Rollback Netcode UDP Server

| Layer | Component | Selection | Rationale |
| :--- | :--- | :--- | :--- |
| **Core Kernel** | Execution Engine | Rust (Tokio, UDP), Next.js 14, WebGL Canvas2D, Python (Isolation Forest) | High performance, memory predictability, and zero-cost abstractions |
| **Governance** | Version Control & CI | GitHub Actions | Automated validation, linting, and dependency monitoring |
| **Documentation** | Technical Specifications | Markdown & Mermaid | Scannable, versioned technical design and system contracts |
