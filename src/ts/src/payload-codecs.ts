import { PayloadTooLargeError, boundRequestBody } from "./request-body.js";

export type JsonScalar = null | boolean | number | string;
export type JsonValue = JsonScalar | readonly JsonValue[] | { readonly [key: string]: JsonValue };
export type JsonObjectMap = Readonly<Record<string, JsonValue>>;
export type HeaderMap = Readonly<Record<string, string>>;

export type PayloadRepresentation =
  | "application/json"
  | "application/problem+json"
  | "application/xml"
  | "application/msgpack"
  | "application/protobuf";

export type PayloadDecoder = (input: Readonly<{
  bytes: Uint8Array;
  mediaType: PayloadRepresentation;
  request: Request;
  routeId?: string;
  protobufMessageType?: string;
}>) => unknown | Promise<unknown>;

export interface PayloadDecoderRegistry {
  readonly xml?: PayloadDecoder;
  readonly messagePack?: PayloadDecoder;
  readonly protobuf?: PayloadDecoder;
}

export interface DecodePayloadOptions {
  readonly maxCompressedBytes: number;
  readonly maxDecodedBytes: number;
  readonly timeoutMs: number;
  readonly decoders?: PayloadDecoderRegistry;
  readonly routeId?: string;
  readonly protobufMessageType?: string;
}

export interface DecodedRequestPayload {
  readonly request: Request;
  readonly headers: HeaderMap;
  readonly representation: PayloadRepresentation;
  readonly bytes: Uint8Array;
  readonly value: unknown;
  readonly jsonPayload?: JsonObjectMap;
}

export class PayloadDecodeError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "PayloadDecodeError";
    this.status = status;
    this.code = code;
  }
}

const UTF8 = new TextDecoder("utf-8", { fatal: true });

export function canonicalHeaderMap(headers: Headers): HeaderMap {
  const result: Record<string, string> = Object.create(null) as Record<string, string>;
  for (const [name, value] of headers.entries()) result[name.toLowerCase()] = value;
  return Object.freeze(result);
}

export function normalizeContentType(raw: string | null): PayloadRepresentation {
  const mediaType = (raw ?? "").split(";", 1)[0]?.trim().toLowerCase() ?? "";
  switch (mediaType) {
    case "application/json":
      return "application/json";
    case "application/problem+json":
      return "application/problem+json";
    case "application/xml":
    case "text/xml":
      return "application/xml";
    case "application/msgpack":
    case "application/x-msgpack":
      return "application/msgpack";
    case "application/protobuf":
    case "application/x-protobuf":
      return "application/protobuf";
    default:
      throw new PayloadDecodeError(415, "unsupported_media_type", "unsupported request content type");
  }
}

function isJsonObject(value: unknown): value is Record<string, JsonValue> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function copySupportedCarriers(source: Request, target: Request): Request {
  const log = (source as Request & { log?: unknown }).log;
  if (log !== undefined) Object.defineProperty(target, "log", { value: log });
  return target;
}

async function readStreamBounded(
  stream: ReadableStream<Uint8Array>,
  maxBytes: number,
  signal: AbortSignal,
  timeoutMs: number,
): Promise<Uint8Array> {
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0 || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new TypeError("body byte limit and timeout must be positive");
  }
  signal.throwIfAborted();
  const reader = stream.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let onAbort: (() => void) | undefined;
  const interrupted = new Promise<never>((_, reject) => {
    onAbort = () => reject(signal.reason ?? new DOMException("request cancelled", "AbortError"));
    signal.addEventListener("abort", onAbort, { once: true });
    timer = setTimeout(() => reject(new DOMException("request body deadline exceeded", "TimeoutError")), timeoutMs);
    if (signal.aborted) onAbort();
  });
  try {
    for (;;) {
      const { done, value } = await Promise.race([reader.read(), interrupted]);
      if (done) break;
      if (!(value instanceof Uint8Array)) throw new TypeError("request body must yield bytes");
      if (value.byteLength > maxBytes - size) throw new PayloadTooLargeError();
      size += value.byteLength;
      chunks.push(value.slice());
    }
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    if (onAbort) signal.removeEventListener("abort", onAbort);
    reader.releaseLock();
  }
  const output = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

async function decodeContentEncoding(request: Request, options: DecodePayloadOptions): Promise<Request> {
  const bounded = await boundRequestBody(request, options.maxCompressedBytes, options.timeoutMs);
  const encoding = (bounded.headers.get("content-encoding") ?? "identity").trim().toLowerCase();
  if (encoding === "" || encoding === "identity") return bounded;
  if (encoding !== "gzip" && encoding !== "deflate") {
    throw new PayloadDecodeError(415, "unsupported_content_encoding", "unsupported request content encoding");
  }
  if (!bounded.body) return bounded;

  let decodedStream: ReadableStream<Uint8Array>;
  try {
    decodedStream = bounded.body.pipeThrough(new DecompressionStream(encoding));
  } catch {
    throw new PayloadDecodeError(400, "invalid_content_encoding", "request body decompression failed");
  }
  let bytes: Uint8Array;
  try {
    bytes = await readStreamBounded(decodedStream, options.maxDecodedBytes, bounded.signal, options.timeoutMs);
  } catch (error) {
    if (error instanceof PayloadTooLargeError) throw error;
    if (error instanceof DOMException && (error.name === "AbortError" || error.name === "TimeoutError")) throw error;
    throw new PayloadDecodeError(400, "invalid_content_encoding", "request body decompression failed");
  }
  const headers = new Headers(bounded.headers);
  headers.delete("content-encoding");
  headers.delete("transfer-encoding");
  headers.set("content-length", String(bytes.byteLength));
  return copySupportedCarriers(bounded, new Request(bounded, { body: bytes, headers }));
}

function requireDecoder(decoder: PayloadDecoder | undefined, code: string): PayloadDecoder {
  if (!decoder) throw new PayloadDecodeError(415, code, "no decoder is configured for the request content type");
  return decoder;
}

function rejectUnsafeXml(bytes: Uint8Array): void {
  // This preflight is defense-in-depth. The configured XML parser must also
  // disable DTD/external-entity resolution because encodings can vary.
  const ascii = new TextDecoder("utf-8", { fatal: false }).decode(bytes).toLowerCase();
  if (ascii.includes("<!doctype") || ascii.includes("<!entity")) {
    throw new PayloadDecodeError(400, "unsafe_xml_document", "XML DTD and entity declarations are not accepted");
  }
}

export async function decodeRequestPayload(
  request: Request,
  options: DecodePayloadOptions,
): Promise<DecodedRequestPayload> {
  const decodedRequest = await decodeContentEncoding(request, options);
  const representation = normalizeContentType(decodedRequest.headers.get("content-type"));
  const bytes = new Uint8Array(await decodedRequest.arrayBuffer());
  if (bytes.byteLength > options.maxDecodedBytes) throw new PayloadTooLargeError();

  let value: unknown;
  try {
    switch (representation) {
      case "application/json":
      case "application/problem+json":
        value = JSON.parse(UTF8.decode(bytes)) as unknown;
        break;
      case "application/xml": {
        rejectUnsafeXml(bytes);
        const decoder = requireDecoder(options.decoders?.xml, "xml_decoder_unavailable");
        value = await decoder({ bytes, mediaType: representation, request: decodedRequest, routeId: options.routeId });
        break;
      }
      case "application/msgpack": {
        const decoder = requireDecoder(options.decoders?.messagePack, "messagepack_decoder_unavailable");
        value = await decoder({ bytes, mediaType: representation, request: decodedRequest, routeId: options.routeId });
        break;
      }
      case "application/protobuf": {
        if (!options.protobufMessageType) {
          throw new PayloadDecodeError(422, "protobuf_message_type_required", "protobuf decoding requires a route message descriptor");
        }
        const decoder = requireDecoder(options.decoders?.protobuf, "protobuf_decoder_unavailable");
        value = await decoder({
          bytes,
          mediaType: representation,
          request: decodedRequest,
          routeId: options.routeId,
          protobufMessageType: options.protobufMessageType,
        });
        break;
      }
    }
  } catch (error) {
    if (error instanceof PayloadDecodeError || error instanceof PayloadTooLargeError) throw error;
    throw new PayloadDecodeError(400, "malformed_request_body", "request body could not be deserialized");
  }

  const headers = canonicalHeaderMap(decodedRequest.headers);
  return {
    request: copySupportedCarriers(decodedRequest, new Request(decodedRequest, { body: bytes })),
    headers,
    representation,
    bytes,
    value,
    ...(isJsonObject(value) ? { jsonPayload: Object.freeze(value) as JsonObjectMap } : {}),
  };
}
