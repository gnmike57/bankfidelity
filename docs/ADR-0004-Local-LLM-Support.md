# ADR-0004: Feature-Gated Local LLM Support

## Status
Accepted

## Context
In previous phases (ADR-0003), local LLM execution (e.g., Ollama, llama.cpp) was explicitly forbidden in the v1 release to preserve deterministic execution, minimize the binary size, and simplify the security model.

However, users operating in high-security, air-gapped environments require the ability to run the Smart Balance Engine and AI completeness checks without sending financial data to cloud providers like Gemini or Mistral.

## Decision
We will implement local LLM support via an HTTP client connecting to a locally running Ollama or llama.cpp server.

This support will be **strictly feature-gated**:
1. It is not bundled in the default binary (requires `cargo build --features local-llm`).
2. It does not embed model weights in the repository or binary.
3. It assumes the user has already provisioned and secured a local inference server.

## Consequences
- **Positive:** Air-gapped deployments can now utilize the Smart Balance Engine.
- **Positive:** No bloat added to the default GUI binary.
- **Negative:** Users must manage their own local inference server lifecycle and hardware acceleration.
- **Negative:** Inference latency will vary wildly based on user hardware, potentially causing the GUI to hang if timeouts are not strictly enforced.

## Implementation Plan
1. Add a `local-llm` feature to `Cargo.toml`.
2. Implement `src/ai/local_llm_client.rs` to interface with the standard OpenAI-compatible `/v1/chat/completions` endpoint exposed by Ollama/llama.cpp.
3. Update `Backend Preferences` to show "Local Inference (Ollama/llama.cpp)" when the feature is enabled.
