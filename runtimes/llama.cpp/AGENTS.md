# Purpose

Own the pinned native Windows llama.cpp binary bundle used to host the local OpenAI-compatible Gemma authoring endpoint.

# Ownership

- `llama-server.exe` and its adjacent llama, ggml, CUDA, OpenMP, implementation, and CPU-variant DLLs form one runtime distribution.
- Auxiliary llama.cpp executables are retained from the same pinned build for diagnosis and model utilities.
- `UPSTREAM.json` records build number, commit, platform, source import, and the external-weights policy.

# Local Contracts

- Treat the binary directory as an atomic upstream bundle; do not replace one executable or DLL independently of a verified compatible build.
- Keep model weights outside the repository.
- Preserve the filenames and adjacency required by `crates/backlot-runtime/src/llama.rs` and Windows dynamic loading.
- The owned server must provide the configured OpenAI-compatible `/v1` surface and a readiness response accepted by the application health check.
- Update `UPSTREAM.json` whenever the bundle changes.

# Work Guidance

- Refresh from an identified upstream build and retain only files needed for the project runtime and diagnostics.
- Coordinate server argument or health-surface changes with `backlot-runtime`, `backlot-app`, configuration, and the live LLM smoke test.

# Verification

- Run `runtimes\llama.cpp\llama-server.exe --version` after a bundle refresh.
- Start the server through the project runtime manager and run the ignored `backlot-llm` live smoke against it.

# Child DOX Index

No child DOX files yet.
