#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# Mini-QUIC Observability: QLOG Diagnostic Analyzer with LLM Integration
# Analyzes QLOG traces for anomalies and root cause diagnosis

import json
from typing import Dict, List, Any, Optional
from dataclasses import dataclass
import statistics

@dataclass
class DiagnosticReport:
    """Structured diagnostic report."""
    root_cause: str
    confidence: float
    severity: str  # "critical", "warning", "info"
    description: str
    suggested_tuning: List[str]
    affected_packets: Optional[List[int]] = None
    time_window_ms: Optional[float] = None


class QlogAnalyzer:
    """Analyzes QLOG traces for network issues."""
    
    def __init__(self):
        self.events = []
        self.packet_loss_threshold = 0.05  # 5% loss is concerning
        self.rtt_variance_threshold = 2.0  # 2x RTT increase is concerning
        
    def load_qlog(self, qlog_data: Dict[str, Any]) -> bool:
        """Load and parse QLOG trace."""
        try:
            if "traces" in qlog_data:
                traces = qlog_data["traces"]
                if traces and "events" in traces[0]:
                    self.events = traces[0]["events"]
                    return True
            elif "events" in qlog_data:
                self.events = qlog_data["events"]
                return True
        except Exception as e:
            print(f"Error loading QLOG: {e}")
            return False
        return False
    
    def analyze_packet_loss(self) -> Optional[DiagnosticReport]:
        """Detect and diagnose packet loss patterns."""
        loss_events = [e for e in self.events if e.get("event") == "packets_lost"]
        
        if not loss_events:
            return None
        
        total_packets = sum(e.get("data", {}).get("packet_numbers", []) for e in self.events 
                           if e.get("event") in ["packet_sent", "packet_received"])
        total_lost = sum(len(e.get("data", {}).get("packet_numbers", [])) for e in loss_events)
        
        loss_rate = total_lost / max(total_packets, 1)
        
        if loss_rate > self.packet_loss_threshold:
            # Check for patterns
            detection_methods = [e.get("data", {}).get("detection_method") for e in loss_events]
            
            if detection_methods.count("packet_threshold") > detection_methods.count("time_threshold"):
                root_cause = "Frequent out-of-order or duplicate packets"
                tuning = [
                    "Increase PTO timeout",
                    "Review ACK delay settings",
                    "Check for network reordering"
                ]
            else:
                root_cause = "Network congestion or persistent packet drops"
                tuning = [
                    "Reduce sending rate",
                    "Increase probe timeout multiplier",
                    "Consider network path change"
                ]
            
            return DiagnosticReport(
                root_cause=root_cause,
                confidence=min(loss_rate * 2, 1.0),
                severity="warning" if loss_rate < 0.1 else "critical",
                description=f"Packet loss rate: {loss_rate * 100:.2f}%",
                suggested_tuning=tuning,
                affected_packets=loss_events,
                time_window_ms=None
            )
        
        return None
    
    def analyze_rtt_behavior(self) -> Optional[DiagnosticReport]:
        """Detect RTT anomalies (bufferbloat, queuing delays)."""
        rtt_events = [e for e in self.events if e.get("event") == "rtt_updated"]
        
        if len(rtt_events) < 2:
            return None
        
        srtt_values = [e.get("data", {}).get("smoothed_rtt") for e in rtt_events]
        srtt_values = [v for v in srtt_values if v is not None]
        
        if not srtt_values or len(srtt_values) < 2:
            return None
        
        avg_srtt = statistics.mean(srtt_values)
        max_srtt = max(srtt_values)
        
        rtt_increase_ratio = max_srtt / max(avg_srtt, 1)
        
        if rtt_increase_ratio > self.rtt_variance_threshold:
            root_cause = "Bufferbloat or excessive queuing detected"
            
            if max_srtt > 100000:  # > 100ms
                severity = "warning"
                tuning = [
                    "Enable ECN (Explicit Congestion Notification)",
                    "Reduce congestion window",
                    "Check queue lengths at bottleneck"
                ]
            else:
                severity = "info"
                tuning = [
                    "Monitor link utilization",
                    "Consider pacing adjustments"
                ]
            
            return DiagnosticReport(
                root_cause=root_cause,
                confidence=0.7,
                severity=severity,
                description=f"RTT spike detected: {avg_srtt/1000:.1f}ms → {max_srtt/1000:.1f}ms",
                suggested_tuning=tuning,
                time_window_ms=max_srtt / 1000
            )
        
        return None
    
    def analyze_congestion_control(self) -> Optional[DiagnosticReport]:
        """Detect congestion control anomalies."""
        cc_events = [e for e in self.events if e.get("event") == "congestion_state_updated"]
        
        if not cc_events:
            return None
        
        # Count state transitions
        states = [e.get("data", {}).get("state") for e in cc_events]
        state_counts = {s: states.count(s) for s in set(states)}
        
        # Detect excessive state thrashing
        if len(cc_events) > 50 and max(state_counts.values()) < len(cc_events) / 3:
            return DiagnosticReport(
                root_cause="Congestion control oscillation",
                confidence=0.6,
                severity="warning",
                description="Frequent state transitions detected",
                suggested_tuning=[
                    "Increase ssthresh calculations",
                    "Smooth window updates",
                    "Increase PTO multiplier"
                ]
            )
        
        # Detect stuck in slow start
        if state_counts.get("slow_start", 0) > len(cc_events) * 0.8:
            return DiagnosticReport(
                root_cause="Connection stuck in Slow Start",
                confidence=0.8,
                severity="warning",
                description="Slow Start phase not completing",
                suggested_tuning=[
                    "Verify handshake completion",
                    "Check for persistent packet loss",
                    "Adjust ssthresh initialization"
                ]
            )
        
        return None
    
    def analyze_flow_control(self) -> Optional[DiagnosticReport]:
        """Detect flow control issues (windowing, buffering)."""
        fc_events = [e for e in self.events if e.get("event") == "max_data_updated"]
        
        if not fc_events:
            return None
        
        # Check for frequent window updates (sign of aggressive buffering)
        if len(fc_events) > 100:
            return DiagnosticReport(
                root_cause="Aggressive buffering or rapid flow control updates",
                confidence=0.5,
                severity="info",
                description=f"High frequency flow control updates: {len(fc_events)}",
                suggested_tuning=[
                    "Reduce flow control window size",
                    "Batch flow control updates",
                    "Monitor receiver buffering"
                ]
            )
        
        return None
    
    def comprehensive_analysis(self) -> List[DiagnosticReport]:
        """Run all analyses and return findings."""
        reports = []
        
        loss_report = self.analyze_packet_loss()
        if loss_report:
            reports.append(loss_report)
        
        rtt_report = self.analyze_rtt_behavior()
        if rtt_report:
            reports.append(rtt_report)
        
        cc_report = self.analyze_congestion_control()
        if cc_report:
            reports.append(cc_report)
        
        fc_report = self.analyze_flow_control()
        if fc_report:
            reports.append(fc_report)
        
        # Sort by severity
        severity_order = {"critical": 0, "warning": 1, "info": 2}
        reports.sort(key=lambda r: severity_order.get(r.severity, 3))
        
        return reports
    
    def export_report_json(self, reports: List[DiagnosticReport]) -> str:
        """Export reports as JSON."""
        report_dicts = []
        for report in reports:
            report_dicts.append({
                "root_cause": report.root_cause,
                "confidence": report.confidence,
                "severity": report.severity,
                "description": report.description,
                "suggested_tuning": report.suggested_tuning,
                "time_window_ms": report.time_window_ms
            })
        
        return json.dumps(report_dicts, indent=2)


class LLMDiagnosticAgent:
    """LLM-powered diagnostic agent (uses OpenAI/Anthropic/HuggingFace)."""
    
    def __init__(self, model_provider: str = "openai", model_name: str = None):
        self.model_provider = model_provider
        self.model_name = model_name or "gpt-4-turbo"
        self.analyzer = QlogAnalyzer()
    
    def diagnose_qlog_trace(self, qlog_data: Dict[str, Any], use_llm: bool = False) -> Dict[str, Any]:
        """Diagnose QLOG trace with optional LLM enhancement."""
        self.analyzer.load_qlog(qlog_data)
        
        # Run structured analysis
        reports = self.analyzer.comprehensive_analysis()
        
        if not use_llm:
            return {
                "diagnostics": self.analyzer.export_report_json(reports),
                "llm_summary": None
            }
        
        # If LLM is enabled, generate natural language summary
        llm_summary = self._generate_llm_summary(reports)
        
        return {
            "diagnostics": self.analyzer.export_report_json(reports),
            "llm_summary": llm_summary
        }
    
    def _generate_llm_summary(self, reports: List[DiagnosticReport]) -> str:
        """Generate human-readable summary (stub - would call LLM API)."""
        if not reports:
            return "No issues detected in QLOG trace."
        
        summary = "**QUIC Connection Diagnostic Summary**\n\n"
        
        for i, report in enumerate(reports, 1):
            summary += f"{i}. **{report.root_cause}** ({report.severity.upper()}, {report.confidence*100:.0f}% confidence)\n"
            summary += f"   - {report.description}\n"
            summary += f"   - Recommendations:\n"
            for tuning in report.suggested_tuning:
                summary += f"     * {tuning}\n"
            summary += "\n"
        
        return summary


if __name__ == "__main__":
    # Example QLOG trace
    example_qlog = {
        "version": "0.3",
        "traces": [{
            "events": [
                {"event": "rtt_updated", "data": {"smoothed_rtt": 50000}},
                {"event": "rtt_updated", "data": {"smoothed_rtt": 150000}},  # Spike
                {"event": "packets_lost", "data": {"packet_numbers": [0, 1, 2], "detection_method": "time_threshold"}},
                {"event": "congestion_state_updated", "data": {"state": "fast_recovery"}},
            ]
        }]
    }
    
    agent = LLMDiagnosticAgent()
    result = agent.diagnose_qlog_trace(example_qlog)
    
    print("Diagnostics:")
    print(result["diagnostics"])
    print("\nLLM Summary:")
    print(result["llm_summary"])
