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
})
