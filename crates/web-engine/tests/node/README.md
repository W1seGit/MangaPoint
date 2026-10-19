# Node smoke test: wasm == native

Checks that the WebAssembly build of the engine makes exactly the files the
native engine makes (an EPUB, an XTC, a colour CBZ and a phone PDF), and that
the small functions (fingerprints, chapter guessing, state file, device list)
give the same answers.

```sh
# 1. native side: generate pages, options and the expected files (deterministic)
cargo run -p web-engine --example make_fixture -- /tmp/engine-fixture

# 2. wasm side: build both flavours; a --target nodejs build lands in <out>/node
crates/web-engine/build-wasm.sh /tmp/engine-wasm      # or build-wasm.ps1 -OutDir ...

# 3. compare (Node 18 or newer)
node crates/web-engine/tests/node/smoke.mjs /tmp/engine-wasm/node /tmp/engine-fixture
```

It exits 0 and prints `all N checks passed` when every file is byte-identical.
