const AES_GCM_IV_BYTES = 12;
const AES_GCM_TAG_BITS = 128;
const ENVELOPE_VERSION = 1;

export interface EncryptedPayloadEnvelope {
  readonly version: 1;
  readonly algorithm: "AES-GCM";
  readonly iv: Uint8Array;
  readonly ciphertext: Uint8Array;
}

export interface PayloadCryptoOptions {
  readonly additionalData?: Uint8Array;
  readonly randomBytes?: (length: number) => Uint8Array;
}

export class PayloadCryptoError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "PayloadCryptoError";
    this.code = code;
  }
}

function defaultRandomBytes(length: number): Uint8Array {
  const value = new Uint8Array(length);
  globalThis.crypto.getRandomValues(value);
  return value;
}

function assertAesGcmKey(key: CryptoKey, usage: KeyUsage): void {
  if (key.algorithm.name !== "AES-GCM") {
    throw new PayloadCryptoError("invalid_crypto_key_algorithm", "payload crypto key must use AES-GCM");
  }
  if (!key.usages.includes(usage)) {
    throw new PayloadCryptoError("invalid_crypto_key_usage", `payload crypto key is not enabled for ${usage}`);
  }
}

/**
 * WebCrypto's BufferSource boundary requires ArrayBuffer-backed data. Copying
 * here deliberately rejects any SharedArrayBuffer provenance and keeps this
 * package portable across Node, browsers, Bun, Deno, and workers with TS 5.9's
 * generic typed-array definitions.
 */
function copyArrayBuffer(value: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(value.byteLength);
  new Uint8Array(buffer).set(value);
  return buffer;
}

function copyBytes(value: Uint8Array): Uint8Array {
  return new Uint8Array(copyArrayBuffer(value));
}

/**
 * Encrypt already-serialized request/response bytes with AES-GCM.
 *
 * Key material is deliberately injected as a CryptoKey. This module never
 * reads environment variables, files, .ores-*.toml, or secret stores.
 */
export async function encryptPayloadAesGcm(
  plaintext: Uint8Array,
  key: CryptoKey,
  options: PayloadCryptoOptions = {},
): Promise<EncryptedPayloadEnvelope> {
  assertAesGcmKey(key, "encrypt");
  const random = options.randomBytes ?? defaultRandomBytes;
  const iv = random(AES_GCM_IV_BYTES);
  if (!(iv instanceof Uint8Array) || iv.byteLength !== AES_GCM_IV_BYTES) {
    throw new PayloadCryptoError("invalid_crypto_nonce", "AES-GCM nonce source must return exactly 12 bytes");
  }
  const algorithm: AesGcmParams = {
    name: "AES-GCM",
    iv: copyArrayBuffer(iv),
    tagLength: AES_GCM_TAG_BITS,
    ...(options.additionalData ? { additionalData: copyArrayBuffer(options.additionalData) } : {}),
  };
  const ciphertext = new Uint8Array(
    await globalThis.crypto.subtle.encrypt(algorithm, key, copyArrayBuffer(plaintext)),
  );
  return Object.freeze({
    version: ENVELOPE_VERSION,
    algorithm: "AES-GCM" as const,
    iv: copyBytes(iv),
    ciphertext,
  });
}

/**
 * Decrypt an AES-GCM payload envelope. Authentication failures are deliberately
 * collapsed into one stable error so callers do not expose cryptographic
 * oracle details to untrusted clients.
 */
export async function decryptPayloadAesGcm(
  envelope: EncryptedPayloadEnvelope,
  key: CryptoKey,
  options: Pick<PayloadCryptoOptions, "additionalData"> = {},
): Promise<Uint8Array> {
  assertAesGcmKey(key, "decrypt");
  if (envelope.version !== ENVELOPE_VERSION || envelope.algorithm !== "AES-GCM") {
    throw new PayloadCryptoError("unsupported_crypto_envelope", "unsupported payload crypto envelope");
  }
  if (!(envelope.iv instanceof Uint8Array) || envelope.iv.byteLength !== AES_GCM_IV_BYTES) {
    throw new PayloadCryptoError("invalid_crypto_nonce", "AES-GCM payload nonce must be exactly 12 bytes");
  }
  if (!(envelope.ciphertext instanceof Uint8Array) || envelope.ciphertext.byteLength < AES_GCM_TAG_BITS / 8) {
    throw new PayloadCryptoError("invalid_ciphertext", "encrypted payload is too short");
  }
  const algorithm: AesGcmParams = {
    name: "AES-GCM",
    iv: copyArrayBuffer(envelope.iv),
    tagLength: AES_GCM_TAG_BITS,
    ...(options.additionalData ? { additionalData: copyArrayBuffer(options.additionalData) } : {}),
  };
  try {
    return new Uint8Array(
      await globalThis.crypto.subtle.decrypt(algorithm, key, copyArrayBuffer(envelope.ciphertext)),
    );
  } catch {
    throw new PayloadCryptoError("payload_authentication_failed", "encrypted payload authentication failed");
  }
}
