# AI-Accelerated SDD Pipeline

.PHONY: setup domain reqs spec review code test

setup:
	cd backend && uv sync

domain:
	claude "Read raw_idea.txt and prompts/p0_domain.txt. Save to docs/00-domain/"

reqs:
	claude "Read docs/00-domain/DOMAIN.md and prompts/p1_requirements.txt. Save to docs/01-requirements/REQUIREMENTS.md"

spec:
	claude "Read docs/01-requirements/REQUIREMENTS.md and prompts/p2_spec.txt. Save to docs/02-spec/SPEC.md"

review:
	claude "Analyze docs/02-spec/SPEC.md using prompts/p3_review.txt. Output verdict."

code:
	claude "Implement SPEC.md into backend/app.py using prompts/p4_code.txt. Use FastAPI/Pydantic."

test:
	cd backend && uv run pytest tests/ -v

all:
	@echo "Running full SDD Vibe..."
	make domain && make reqs && make spec && make review && make code
