import socket
import time
import argparse

def run_load(clients: int, host: str, port: int):
    print(f"Launching {clients} synthetic client sockets targeted at {host}:{port}...")
    print("Simulated RTT & Jitter: TO BE MEASURED under real network interface")

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--clients", type=int, default=100)
    parser.add_argument("--host", type=str, default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9000)
    args = parser.parse_args()
    run_load(args.clients, args.host, args.port)
