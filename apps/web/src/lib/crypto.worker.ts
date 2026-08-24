/// <reference lib="webworker" />

const scope = self as DedicatedWorkerGlobalScope;

scope.onmessage = () => {
  scope.postMessage({
    ok: false,
    code: "crypto_release_blocked",
    message: "OpenMLS WASM is disabled until the release gate passes."
  });
};

export {};
