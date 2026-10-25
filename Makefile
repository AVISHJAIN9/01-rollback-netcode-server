# Unified Makefile for Rollback Netcode Server
.PHONY: setup test verify dev clean bench lint docker-up docker-down dev-down

setup:
	@echo "==> Setting up development environment..."
	@pip3 install -r aiml/requirements.txt
	@echo "==> Setup complete."

test-integration:
	@echo "==> Starting Postgres and Redis services..."
	@docker-compose -f infra/docker-compose.yml up -d postgres redis
	@echo ""
	@echo "==> Checking service status..."
	@docker-compose -f infra/docker-compose.yml ps postgres redis
	@echo ""
	@echo "==> Waiting for services to be ready..."
	@max_attempts=30; attempt=0; \
	while [ $$attempt -lt $$max_attempts ]; do \
		if docker-compose -f infra/docker-compose.yml exec postgres pg_isready -U postgres >/dev/null 2>&1 && \
		   docker-compose -f infra/docker-compose.yml exec redis redis-cli ping >/dev/null 2>&1; then \
			echo "✓ Services ready"; \
			break; \
		fi; \
		attempt=$$(($$attempt + 1)); \
		sleep 1; \
	done
	@echo ""
	@echo "==> Running integration tests (ALLOW_SKIP_INTEGRATION unset: tests will fail if services unavailable)..."
	@cd backend && ALLOW_SKIP_INTEGRATION=0 cargo test --test postgres_redis_integration -- --nocapture --test-threads=1
	@result=$$?
	@echo ""
	@echo "==> Shutting down services..."
	@docker-compose -f infra/docker-compose.yml down
	@echo ""
	@exit $$result

test:
	@echo "==> Running Rust Backend Test Suite..."
	@cd backend && cargo test
	@echo "==> [Gate G1 & G2] 1,000,000 Step Determinism & Golden State Verification..."
	@python3 tests/test_determinism.py
	@python3 tests/test_golden_determinism.py
	@python3 tests/test_rollback.py
	@echo "==> [Gate G3] Multiplayer Session & Reconnect Verification..."
	@python3 tests/test_multiplayer_session.py
	@echo "==> [Gate G4] Security, Rate Limiting & Fuzzing Verification..."
	@python3 tests/test_security_fuzzing.py
	@echo "==> [Gate G5] Network Chaos & Packet Loss Redundancy Verification..."
	@python3 tests/test_chaos_network.py
	@echo "==> [Gate G6] AIML Models, Safety Guardrails & MLOps Verification..."
	@python3 aiml/eval/evaluate_all_models.py
	@echo "==> [Gate G7] E2E Multiplayer Lifecycle Verification..."
	@python3 tests/test_e2e_flow.py
	@echo "==> [Gate G8] Client SDK Smoke Verification..."
	@python3 tests/test_sdk_smoke.py
	@echo ""
	@echo "=================================================================="
	@echo " ALL RELEASE GATES G1 THROUGH G8 VERIFIED 100% GREEN."
	@echo "=================================================================="

lint: check-floats
	@echo "==> Checking Rust Backend (clippy & fmt)..."
	@cd backend && cargo fmt --check && cargo clippy -- -D warnings
	@echo "==> Checking Python syntax..."
	@python3 -m py_compile tests/*.py aiml/**/*.py
	@echo "==> Syntax clean."

check-floats:
	@echo "==> Verifying Zero-Float Invariant in math/ and simulation/..."
	@! grep -rnE "\b(f32|f64)\b" backend/src/math/ backend/src/simulation/
	@echo "==> [PASS] Zero IEEE-754 floats in simulation code."

bench:
	@echo "==> Executing simulation benchmarks..."
	@python3 tests/test_determinism.py

dev:
	@echo "==> Starting local development stack..."
	@docker-compose -f infra/docker-compose.yml up -d
	@echo "==> Stack online. Frontend: http://localhost:3000, Server: UDP 0.0.0.0:9000"

dev-down:
	@echo "==> Stopping development stack..."
	@docker-compose -f infra/docker-compose.yml down
	@echo "==> Stack stopped."

verify:
	@bash scripts/verify.sh

clean:
	@echo "==> Cleaning build artifacts and cache..."
	@find . -type d -name "__pycache__" -exec rm -rf {} +
	@rm -rf frontend/.next
	@echo "==> Clean complete."
