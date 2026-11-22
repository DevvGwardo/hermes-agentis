---
name: uniichat-recall-jev
description: Audit uniichat-recall memory accuracy and project context-rot decay using TypeSafe AI's System One model (Jev) so Hermes knows exactly when to compact or start a fresh session.
version: 1.0.0
author: Hermes Agent
license: MIT
platforms: [macos, linux, windows]
metadata:
  hermes:
    tags: [memory, recall, uniichat-recall, typesafe, jev, context-rot, calibration]
    category: memory
    related_skills: [memory-hygiene, session-handoff]
    references:
      - https://github.com/DevvGwardo/uniichat-recall
      - https://api.typesafe.ai/v1/systemone
    config:
      - key: typesafe.api_key
        env: TYPESAFE_API_KEY
        description: "TypeSafe AI API key for System One (Jev) calibrated decision evaluations"
        prompt: "Enter your TypeSafe AI API key (TYPESAFE_API_KEY)"
        url: "https://typesafe.ai"
      - key: typesafe.model
        env: TYPESAFE_JEV_MODEL
        description: "TypeSafe System One model identifier (default: jev-latest)"
        default: "jev-latest"
      - key: uniichat_recall.threshold
        description: "Minimum calibrated recall accuracy before recommending a new session (default: 0.78)"
        default: "0.78"
requires_toolsets: [web]
---

# Uniichat Recall Accuracy & Session Freshness (TypeSafe Jev)

## Purpose
Pair **[DevvGwardo/uniichat-recall](https://github.com/DevvGwardo/uniichat-recall)** (append-only UniiChat JSONL log + Coverage/BM25-lite two-tier evidence + zero-leak abstaining negative gate) with **TypeSafe AI's System One model (`Jev`)** to measure live recall grounding accuracy and project when session context rot will cross your reset threshold.

Unlike generative LLMs, **Jev** evaluates a shared state against parallel typed primitives (`noul`, `score`, `choice`) and returns RLCD-calibrated probabilities (Expected Calibration Error ~0.07) without prose generation overhead.

## Reference Benchmarks (`uniichat-recall`)
Measured on a 20-day UniiChat log (120 gold queries, strict `hit@1`):
- **Coverage + BM25-lite re-rank (shipped):** `0.933 hit@1` · `0.975 hit@3` · `1.000 hit@6` · `16.7 ms` p50 ranking · `0.86 s` e2e (33× faster than 28.5 s Hermes multi-hop walk)
- **Keyword first-pass:** `0.858 hit@1` · `0.967 hit@3` · `1.000 hit@6`
- **BM25 only:** `0.742 hit@1` · `0.875 hit@3` · `1.000 hit@6`
- **Negative gate:** `20/20` adversarial queries abstained (`100%` zero-leak); outbound transcripts redact `sk-*`, `ghp_*`, `gho_*`, `vck_*`, and `apikey_*`.
- **Paraphrase recall:** Lexical ranking drops on rephrased wording (`3/6` frozen 20-day bench); optional query expansion recovers paraphrase hits (`5/8 -> 6/8`).

## TypeSafe Jev Context Limits
- **Endpoint:** `POST https://api.typesafe.ai/v1/systemone`
- **Model:** `jev-latest` (or pinned release e.g. `jev-1.14.0`)
- **Per-request context cap:** `64,000` tokens (shared `state` + all questions)
- **Per-question context cap:** `32,000` tokens (shared `state` + longest single question)
- Keep `state` pruned to compact turn excerpts + retrieved `uniichat-recall` evidence spans; irrelevant noise degrades calibrated recall precision.

## When to Use
- Enable the **Recall** accuracy monitor toggle during long multi-turn coding, research, or ops sessions.
- Run a Jev audit whenever a session exceeds 10+ turns, crosses 12k tokens, shifts topics sharply, or relies heavily on paraphrased references to earlier decisions.
- Start a **new session** with a clean handoff summary once projected recall accuracy drops below the configured threshold (default `78%`) or Jev's `session_action` returns `start_new_session`.

## Jev Evaluation Payload
Before transmitting any excerpt to `api.typesafe.ai`, redact credential-shaped tokens (`sk-*`, `ghp_*`, `vck_*`, `apikey_*`).

```bash
curl --request POST \
  --url https://api.typesafe.ai/v1/systemone \
  --header "Authorization: Bearer $TYPESAFE_API_KEY" \
  --header "Content-Type: application/json" \
  --data '{
    "model": "jev-latest",
    "state": {
      "provider": "uniichat-recall",
      "retrieval_mode": "coverage-rerank",
      "query_expansion": true,
      "turn_count": 14,
      "estimated_tokens": 9840,
      "recent_turns": [
        { "role": "user", "excerpt": "..." },
        { "role": "assistant", "excerpt": "..." }
      ]
    },
    "questions": {
      "recall_grounded": {
        "type": "noul",
        "instructions": "Is the assistant accurately grounding its replies in prior session facts and uniichat-recall evidence without losing earlier constraints?"
      },
      "abstention_safe": {
        "type": "noul",
        "instructions": "Does the session avoid hallucinated recalls and respect abstention when facts are absent from context?"
      },
      "context_coherence": {
        "type": "score",
        "instructions": "Rate how crisp and unpolluted the active context window remains for uniichat-recall retrieval.",
        "criteria": ["degraded", "drifting", "nominal", "sharp"]
      },
      "session_action": {
        "type": "choice",
        "instructions": "Recommend the best session hygiene action right now.",
        "criteria": {
          "continue": "Recall accuracy is healthy; keep working in this session.",
          "compact": "Topic drift or token density is rising; summarize/prune soon.",
          "start_new_session": "Recall accuracy is projected below threshold; hand off key facts and start a new session."
        }
      }
    }
  }'
```

## Decision Thresholds & Session Handoff Workflow
1. **`>= 85%` (Fresh / Nominal):** Continue in the current session. `uniichat-recall` coverage + BM25-lite ranking operates near its `93.3%` hit@1 ceiling.
2. **`75% – 84%` (Watch):** Context density or topic entropy is rising. Enable `uniichat-recall` query expansion for paraphrased lookups or prepare to hand off soon.
3. **`< 78%` (or user threshold) / `start_new_session`:**
   - Capture active decisions, file paths, and open TODOs into a concise handoff note.
   - Start a fresh Hermes session (`New session`) so `uniichat-recall` keeps historical records in the append-only JSONL log while the active context window returns to peak accuracy.

## Pitfalls to Avoid
- **Never** send unredacted API keys, bearer tokens, or `.env` dumps inside the Jev `state` object.
- **Never** exceed Jev's `32k` per-question context budget — slice long transcripts to recent turns plus top retrieved spans.
- **Never** treat a missed lexical match as permission to guess; preserve `uniichat-recall`'s abstaining negative gate.
