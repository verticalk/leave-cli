import { describe, expect, it } from "vitest";
import { putEncryptedEnvelope } from "./offline-store";

describe("encrypted offline cache", () => {
  it("rejects empty ciphertext before touching IndexedDB", async () => {
    await expect(putEncryptedEnvelope("event", "workspace", new Uint8Array())).rejects.toThrow(
      "empty envelope"
    );
  });
});
