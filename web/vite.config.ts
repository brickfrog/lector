import { defineConfig } from "vite"

// Cross-origin isolation enables SharedArrayBuffer, which onnxruntime-web needs
// to run the wasm backend multi-threaded. Without these headers it silently
// falls back to a single thread (~3-4x slower). COEP=credentialless (rather than
// require-corp) keeps cross-origin assets like the espeak wasm loadable.
const coiHeaders = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "credentialless",
}

export default defineConfig({
  server: { headers: coiHeaders },
  preview: { headers: coiHeaders },
  // Vite's esbuild pre-bundling mangles onnxruntime-web's internal worker/wasm
  // spawning (the threaded build dies on load). Leave it unbundled.
  optimizeDeps: { exclude: ["onnxruntime-web"] },
  worker: { format: "es" },
})
