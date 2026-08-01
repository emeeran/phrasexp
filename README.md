# phrasexp

Built with the Spec-Driven Development (SDD) pipeline. See `CLAUDE.md` for the workflow.

## Layout
- `docs/` — SDD artifacts (domain → requirements → spec → review → design → testing → integration → deployment)
- `prompts/` — pipeline prompt templates (p0–p4)
- `backend/` — FastAPI app, managed with `uv`

## Quickstart
1. `make setup` — sync dependencies
2. Edit `raw_idea.txt`
3. `make all` — run the SDD pipeline
