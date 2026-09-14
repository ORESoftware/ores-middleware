import assert from "node:assert/strict";
import { test } from "node:test";

import {
  PayloadCryptoError,
  decryptPayloadAesGcm,
  encryptPayloadAesGcm
} from "../dist/body-crypto.js";

test("AES-GCM payload encryption round-trips with authenticated context", async () => {
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
  const plaintext = new TextEncoder().encode('{"ok":true}');
  const aad = new TextEncoder().encode("POST\n/v1/items\napplication/json");
  const encrypted = await encryptPayloadAesGcm(plaintext, key, {
    additionalData: aad,
    randomBytes: (length) => Uint8Array.from({ length }, (_, index) => index + 1)
  });
  assert.equal(encrypted.version, 1);
  assert.equal(encrypted.algorithm, "AES-GCM");
  assert.equal(encrypted.iv.byteLength, 12);
  assert.notDeepEqual([...encrypted.ciphertext], [...plaintext]);
  const decrypted = await decryptPayloadAesGcm(encrypted, key, { additionalData: aad });
  assert.deepEqual([...decrypted], [...plaintext]);
});

test("AES-GCM rejects tampering without exposing crypto-oracle detail", async () => {
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
  const encrypted = await encryptPayloadAesGcm(new Uint8Array([1, 2, 3]), key);
  const tampered = {
    ...encrypted,
    ciphertext: Uint8Array.from(encrypted.ciphertext, (value, index) => index === 0 ? value ^ 1 : value)
  };
  await assert.rejects(
    () => decryptPayloadAesGcm(tampered, key),
    (error) => error instanceof PayloadCryptoError && error.code === "payload_authentication_failed"
  );
});

test("nonce source must return exactly twelve bytes", async () => {
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
  await assert.rejects(
    () => encryptPayloadAesGcm(new Uint8Array([1]), key, { randomBytes: () => new Uint8Array(8) }),
    (error) => error instanceof PayloadCryptoError && error.code === "invalid_crypto_nonce"
  );
});
